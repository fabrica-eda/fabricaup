use anyhow::{Context, Result, bail};
use clap::{Parser, Subcommand};
use fabricaup_core::{
    DEFAULT_ORG, DEFAULT_TOOL, Layout, target_triple, validate_tool, validate_version,
};
use reqwest::blocking::Client;
use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::fs::{self, File};
use std::io::{self, Read};
use std::path::{Path, PathBuf};

#[derive(Debug, Parser)]
#[command(
    name = "fabricaup",
    version,
    about = "The Fabrica EDA toolchain manager"
)]
struct Cli {
    /// Fabrica tool to manage.
    #[arg(long, global = true, env = "FABRICAUP_TOOL", default_value = DEFAULT_TOOL)]
    tool: String,

    /// GitHub release repository; defaults to fabrica-eda/<tool>.
    #[arg(long, global = true, env = "FABRICA_DIST_REPO")]
    repo: Option<String>,

    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Download and select a tool release.
    Install {
        /// Release tag, or "latest".
        #[arg(default_value = "latest")]
        version: String,
        /// Replace the downloaded copy if it already exists.
        #[arg(long)]
        force: bool,
    },
    /// Install and select the newest release.
    Update,
    /// Select an already installed release.
    Default { version: String },
    /// Show installed releases.
    List,
    /// Print the active tool executable path.
    Which,
    /// Remove an inactive release.
    Uninstall { version: String },
}

#[derive(Debug, Deserialize)]
struct Release {
    tag_name: String,
    assets: Vec<Asset>,
}

#[derive(Debug, Deserialize)]
struct Asset {
    name: String,
    browser_download_url: String,
}

fn main() {
    if let Err(error) = run() {
        eprintln!("error: {error:#}");
        std::process::exit(1);
    }
}

fn run() -> Result<()> {
    let cli = Cli::parse();
    validate_tool(&cli.tool)?;
    let repo = cli
        .repo
        .unwrap_or_else(|| format!("{DEFAULT_ORG}/{}", cli.tool));
    validate_repo(&repo)?;
    let layout = Layout::discover()?;

    match cli.command {
        Command::Install { version, force } => install(&layout, &cli.tool, &repo, &version, force),
        Command::Update => install(&layout, &cli.tool, &repo, "latest", true),
        Command::Default { version } => {
            let files = layout.activate(&cli.tool, &version)?;
            println!(
                "default {} toolchain set to {version} ({})",
                cli.tool,
                files.join(", ")
            );
            Ok(())
        }
        Command::List => {
            let default = layout.default_version(&cli.tool)?;
            for version in layout.installed(&cli.tool)? {
                let marker = if default.as_deref() == Some(&version) {
                    " (default)"
                } else {
                    ""
                };
                println!("{} {version}{marker}", cli.tool);
            }
            Ok(())
        }
        Command::Which => {
            let binary = tool_binary(&cli.tool);
            let path = layout.bin.join(&binary);
            if !path.is_file() {
                bail!(
                    "no active {} executable; run `fabricaup install --tool {}`",
                    cli.tool,
                    cli.tool
                );
            }
            println!("{}", path.display());
            Ok(())
        }
        Command::Uninstall { version } => {
            layout.remove(&cli.tool, &version)?;
            println!("uninstalled {} {version}", cli.tool);
            Ok(())
        }
    }
}

fn validate_repo(repo: &str) -> Result<()> {
    let mut parts = repo.split('/');
    let valid_part = |part: &str| {
        !part.is_empty()
            && part
                .chars()
                .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_' | '.'))
    };
    match (parts.next(), parts.next(), parts.next()) {
        (Some(owner), Some(name), None) if valid_part(owner) && valid_part(name) => Ok(()),
        _ => bail!("invalid GitHub repository {repo:?}; expected owner/name"),
    }
}

fn tool_binary(tool: &str) -> String {
    if cfg!(windows) {
        format!("{tool}.exe")
    } else {
        tool.to_owned()
    }
}

fn install(layout: &Layout, tool: &str, repo: &str, requested: &str, force: bool) -> Result<()> {
    if requested != "latest" {
        validate_version(requested)?;
    }
    layout.initialize()?;
    let client = Client::builder()
        .user_agent(concat!("fabricaup/", env!("CARGO_PKG_VERSION")))
        .build()?;
    let release = fetch_release(&client, repo, requested)?;
    validate_version(&release.tag_name)?;
    let version = release.tag_name;
    let destination = layout.toolchain_dir(tool, &version)?;

    if destination.exists() && !force {
        layout.activate(tool, &version)?;
        println!("{tool} {version} is already installed and is now the default");
        return Ok(());
    }

    let target = target_triple()?;
    let extension = if cfg!(windows) { "zip" } else { "tar.gz" };
    let asset_base = format!("{tool}-{target}");
    let archive_name = format!("{asset_base}.{extension}");
    let checksum_name = format!("{asset_base}.sha256");
    let archive = find_asset(&release.assets, &archive_name)?;
    let checksum = find_asset(&release.assets, &checksum_name)?;

    println!("downloading {archive_name}");
    let temp = tempfile::tempdir_in(&layout.home)?;
    let archive_path = temp.path().join(&archive_name);
    download_to(&client, &archive.browser_download_url, &archive_path)?;
    let expected = download_text(&client, &checksum.browser_download_url)?;
    verify_sha256(&archive_path, &expected)?;

    let unpacked = temp.path().join("unpacked");
    fs::create_dir(&unpacked)?;
    extract(&archive_path, &unpacked, extension)?;
    let binary = tool_binary(tool);
    let source_binary = find_binary(&unpacked, &binary)?;

    let staged = temp.path().join("toolchain");
    let staged_bin = staged.join("bin");
    fs::create_dir_all(&staged_bin)?;
    fs::copy(&source_binary, staged_bin.join(&binary))
        .with_context(|| format!("failed to stage {}", source_binary.display()))?;
    if destination.exists() {
        fs::remove_dir_all(&destination)
            .with_context(|| format!("failed to replace {}", destination.display()))?;
    }
    fs::create_dir_all(
        destination
            .parent()
            .context("toolchain destination has no parent directory")?,
    )?;
    fs::rename(&staged, &destination)
        .with_context(|| format!("failed to install {}", destination.display()))?;
    let files = layout.activate(tool, &version)?;
    println!("installed {tool} {version} ({})", files.join(", "));
    println!("binaries are in {}", layout.bin.display());
    Ok(())
}

fn fetch_release(client: &Client, repo: &str, requested: &str) -> Result<Release> {
    let endpoint = if requested == "latest" {
        format!("https://api.github.com/repos/{repo}/releases/latest")
    } else {
        format!("https://api.github.com/repos/{repo}/releases/tags/{requested}")
    };
    let mut request = client.get(endpoint);
    if let Some(token) = std::env::var_os("GITHUB_TOKEN").or_else(|| std::env::var_os("GH_TOKEN")) {
        request = request.bearer_auth(token.to_string_lossy());
    }
    request
        .send()?
        .error_for_status()
        .with_context(|| format!("release {requested:?} was not found in {repo}"))?
        .json()
        .context("GitHub returned an invalid release response")
}

fn find_asset<'a>(assets: &'a [Asset], name: &str) -> Result<&'a Asset> {
    assets
        .iter()
        .find(|asset| asset.name == name)
        .with_context(|| format!("release does not contain required asset {name}"))
}

fn download_to(client: &Client, url: &str, path: &Path) -> Result<()> {
    let mut response = client.get(url).send()?.error_for_status()?;
    let mut output = File::create(path)?;
    io::copy(&mut response, &mut output)?;
    Ok(())
}

fn download_text(client: &Client, url: &str) -> Result<String> {
    client
        .get(url)
        .send()?
        .error_for_status()?
        .text()
        .context("failed to read checksum")
}

fn verify_sha256(path: &Path, checksum_file: &str) -> Result<()> {
    let expected = checksum_file
        .split_whitespace()
        .next()
        .context("checksum file is empty")?;
    if expected.len() != 64 || !expected.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        bail!("release contains an invalid SHA-256 checksum");
    }

    let mut file = File::open(path)?;
    let mut hasher = Sha256::new();
    let mut buffer = [0_u8; 64 * 1024];
    loop {
        let count = file.read(&mut buffer)?;
        if count == 0 {
            break;
        }
        hasher.update(&buffer[..count]);
    }
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let digest = hasher.finalize();
    let mut actual = String::with_capacity(digest.len() * 2);
    for byte in digest.iter().copied() {
        actual.push(HEX[(byte >> 4) as usize] as char);
        actual.push(HEX[(byte & 0x0f) as usize] as char);
    }
    if !actual.eq_ignore_ascii_case(expected) {
        bail!("checksum mismatch for {}", path.display());
    }
    Ok(())
}

fn extract(archive: &Path, destination: &Path, extension: &str) -> Result<()> {
    match extension {
        "tar.gz" => {
            let decoder = flate2::read::GzDecoder::new(File::open(archive)?);
            tar::Archive::new(decoder)
                .unpack(destination)
                .context("failed to unpack tar archive")?;
        }
        "zip" => {
            let mut archive = zip::ZipArchive::new(File::open(archive)?)?;
            for index in 0..archive.len() {
                let mut entry = archive.by_index(index)?;
                let relative = entry
                    .enclosed_name()
                    .context("zip archive contains an unsafe path")?;
                let output = destination.join(relative);
                if entry.is_dir() {
                    fs::create_dir_all(&output)?;
                } else {
                    if let Some(parent) = output.parent() {
                        fs::create_dir_all(parent)?;
                    }
                    let mut file = File::create(&output)?;
                    io::copy(&mut entry, &mut file)?;
                }
            }
        }
        _ => bail!("unsupported archive format: {extension}"),
    }
    Ok(())
}

fn find_binary(root: &Path, name: &str) -> Result<PathBuf> {
    for candidate in [root.join(name), root.join("bin").join(name)] {
        if is_regular_file(&candidate) {
            return Ok(candidate);
        }
    }
    for entry in fs::read_dir(root)? {
        let entry = entry?;
        if entry.file_type()?.is_dir() {
            for candidate in [entry.path().join(name), entry.path().join("bin").join(name)] {
                if is_regular_file(&candidate) {
                    return Ok(candidate);
                }
            }
        }
    }
    bail!("archive does not contain the {name} executable")
}

fn is_regular_file(path: &Path) -> bool {
    fs::symlink_metadata(path)
        .map(|metadata| metadata.file_type().is_file())
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validates_repository_names() {
        assert!(validate_repo("fabrica-eda/texo").is_ok());
        assert!(validate_repo("missing-owner").is_err());
        assert!(validate_repo("owner/repo/extra").is_err());
        assert!(validate_repo("owner/../repo").is_err());
    }

    #[test]
    fn verifies_checksum() -> Result<()> {
        let temp = tempfile::tempdir()?;
        let file = temp.path().join("asset");
        fs::write(&file, b"texo")?;
        verify_sha256(
            &file,
            "7180eae482801dc5dcb0630b5d9defaf6a3ff56acc5b826ca12a5a1194a0190d  asset",
        )?;
        assert!(verify_sha256(&file, &"0".repeat(64)).is_err());
        Ok(())
    }

    #[test]
    fn finds_released_texo_layout() -> Result<()> {
        let temp = tempfile::tempdir()?;
        let package = temp.path().join("texo-x86_64-unknown-linux-gnu");
        fs::create_dir_all(&package)?;
        fs::write(package.join("texo"), b"binary")?;
        assert_eq!(find_binary(temp.path(), "texo")?, package.join("texo"));
        Ok(())
    }

    #[test]
    fn finds_bin_directory_layout() -> Result<()> {
        let temp = tempfile::tempdir()?;
        fs::create_dir_all(temp.path().join("package/bin"))?;
        fs::write(temp.path().join("package/bin/texo"), b"binary")?;
        assert_eq!(
            find_binary(temp.path(), "texo")?,
            temp.path().join("package/bin/texo")
        );
        Ok(())
    }
}
