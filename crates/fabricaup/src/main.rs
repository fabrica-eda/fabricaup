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

const SELF_REPO: &str = "fabrica-eda/fabricaup";

#[derive(Debug, Parser)]
#[command(
    name = "fabricaup",
    version,
    about = "The Fabrica EDA toolchain manager"
)]
struct Cli {
    /// Fabrica tool to manage; not used by self commands.
    #[arg(long, global = true, env = "FABRICAUP_TOOL", default_value = DEFAULT_TOOL)]
    tool: String,

    /// GitHub release repository; defaults to fabrica-eda/<tool>, or fabricaup for self update.
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
    /// Manage the fabricaup executable.
    #[command(name = "self")]
    Self_ {
        #[command(subcommand)]
        command: SelfCommand,
    },
}

#[derive(Debug, Subcommand)]
enum SelfCommand {
    /// Update fabricaup to the latest release.
    Update,
}

#[derive(Debug, Deserialize)]
struct Release {
    tag_name: String,
    #[serde(default)]
    draft: bool,
    #[serde(default)]
    prerelease: bool,
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
    let Cli {
        tool,
        repo,
        command,
    } = Cli::parse();
    if matches!(
        &command,
        Command::Self_ {
            command: SelfCommand::Update
        }
    ) {
        let repo = repo.unwrap_or_else(|| SELF_REPO.to_owned());
        validate_repo(&repo)?;
        return self_update(&repo);
    }

    validate_tool(&tool)?;
    let repo = repo.unwrap_or_else(|| format!("{DEFAULT_ORG}/{tool}"));
    validate_repo(&repo)?;
    let layout = Layout::discover()?;

    match command {
        Command::Install { version, force } => install(&layout, &tool, &repo, &version, force),
        Command::Update => install(&layout, &tool, &repo, "latest", true),
        Command::Default { version } => {
            let files = layout.activate(&tool, &version)?;
            println!(
                "default {} toolchain set to {version} ({})",
                tool,
                files.join(", ")
            );
            Ok(())
        }
        Command::List => {
            let default = layout.default_version(&tool)?;
            for version in layout.installed(&tool)? {
                let marker = if default.as_deref() == Some(&version) {
                    " (default)"
                } else {
                    ""
                };
                println!("{tool} {version}{marker}");
            }
            Ok(())
        }
        Command::Which => {
            let binary = tool_binary(&tool);
            let path = layout.bin.join(&binary);
            if !path.is_file() {
                bail!(
                    "no active {} executable; run `fabricaup install --tool {}`",
                    tool,
                    tool
                );
            }
            println!("{}", path.display());
            Ok(())
        }
        Command::Uninstall { version } => {
            layout.remove(&tool, &version)?;
            println!("uninstalled {tool} {version}");
            Ok(())
        }
        Command::Self_ { .. } => unreachable!("self-update was handled before layout discovery"),
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

fn self_update(repo: &str) -> Result<()> {
    let client = Client::builder()
        .user_agent(concat!("fabricaup/", env!("CARGO_PKG_VERSION")))
        .build()?;
    let target = target_triple()?;
    let extension = if cfg!(windows) { "zip" } else { "tar.gz" };
    let archive_name = format!("fabricaup-{target}.{extension}");
    let checksum_name = format!("{archive_name}.sha256");
    let release = fetch_release(&client, repo, "latest", &archive_name, &checksum_name)?;
    let latest = stable_version(&release.tag_name)
        .with_context(|| format!("invalid fabricaup release tag {}", release.tag_name))?;
    let current_tag = format!("v{}", env!("CARGO_PKG_VERSION"));
    let current = stable_version(&current_tag).context("invalid compiled fabricaup version")?;

    match latest.cmp(&current) {
        std::cmp::Ordering::Less => {
            println!(
                "fabricaup {} is newer than the latest release {}; no update needed",
                env!("CARGO_PKG_VERSION"),
                release.tag_name
            );
            return Ok(());
        }
        std::cmp::Ordering::Equal => {
            println!(
                "fabricaup {} is already up to date",
                env!("CARGO_PKG_VERSION")
            );
            return Ok(());
        }
        std::cmp::Ordering::Greater => {}
    }

    println!("downloading {archive_name}");
    let temp = tempfile::tempdir()?;
    let binary = tool_binary("fabricaup");
    let replacement = download_release_binary(
        &client,
        &release,
        &archive_name,
        &checksum_name,
        extension,
        &binary,
        temp.path(),
    )?;
    verify_replacement_version(&replacement, &release.tag_name)?;
    self_replace::self_replace(&replacement).with_context(|| {
        format!(
            "failed to replace {}; check its permissions",
            std::env::current_exe()
                .map(|path| path.display().to_string())
                .unwrap_or_else(|_| "the current executable".to_owned())
        )
    })?;
    println!(
        "updated fabricaup from {} to {}",
        env!("CARGO_PKG_VERSION"),
        release.tag_name.trim_start_matches('v')
    );
    Ok(())
}

fn verify_replacement_version(binary: &Path, expected_tag: &str) -> Result<()> {
    let output = std::process::Command::new(binary)
        .arg("--version")
        .output()
        .with_context(|| format!("failed to run downloaded {}", binary.display()))?;
    if !output.status.success() {
        bail!("downloaded fabricaup executable failed its version check");
    }
    let actual = String::from_utf8(output.stdout).context("fabricaup version is not UTF-8")?;
    let expected = format!("fabricaup {}", expected_tag.trim_start_matches('v'));
    if actual.trim() != expected {
        bail!(
            "downloaded executable reported version {:?}, expected {expected:?}",
            actual.trim()
        );
    }
    Ok(())
}

fn install(layout: &Layout, tool: &str, repo: &str, requested: &str, force: bool) -> Result<()> {
    if requested != "latest" {
        validate_version(requested)?;
    }
    layout.initialize()?;
    let client = Client::builder()
        .user_agent(concat!("fabricaup/", env!("CARGO_PKG_VERSION")))
        .build()?;
    let target = target_triple()?;
    let extension = if cfg!(windows) { "zip" } else { "tar.gz" };
    let asset_base = format!("{tool}-{target}");
    let archive_name = format!("{asset_base}.{extension}");
    let checksum_name = format!("{asset_base}.sha256");
    let release = fetch_release(&client, repo, requested, &archive_name, &checksum_name)?;
    validate_version(&release.tag_name)?;
    let version = release.tag_name.clone();
    let destination = layout.toolchain_dir(tool, &version)?;

    if destination.exists() && !force {
        layout.activate(tool, &version)?;
        println!("{tool} {version} is already installed and is now the default");
        return Ok(());
    }

    println!("downloading {archive_name}");
    let temp = tempfile::tempdir_in(&layout.home)?;
    let binary = tool_binary(tool);
    let source_binary = download_release_binary(
        &client,
        &release,
        &archive_name,
        &checksum_name,
        extension,
        &binary,
        temp.path(),
    )?;

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

fn download_release_binary(
    client: &Client,
    release: &Release,
    archive_name: &str,
    checksum_name: &str,
    extension: &str,
    binary: &str,
    temp: &Path,
) -> Result<PathBuf> {
    let archive = find_asset(&release.assets, archive_name)?;
    let checksum = find_asset(&release.assets, checksum_name)?;
    let archive_path = temp.join(archive_name);
    download_to(client, &archive.browser_download_url, &archive_path)?;
    let expected = download_text(client, &checksum.browser_download_url)?;
    verify_sha256(&archive_path, &expected)?;

    let unpacked = temp.join("unpacked");
    fs::create_dir(&unpacked)?;
    extract(&archive_path, &unpacked, extension)?;
    find_binary(&unpacked, binary)
}

fn release_request(client: &Client, endpoint: String) -> reqwest::blocking::RequestBuilder {
    let mut request = client.get(endpoint);
    let token = std::env::var("GITHUB_TOKEN")
        .ok()
        .filter(|token| !token.is_empty())
        .or_else(|| {
            std::env::var("GH_TOKEN")
                .ok()
                .filter(|token| !token.is_empty())
        });
    if let Some(token) = token {
        request = request.bearer_auth(token);
    }
    request
}

fn fetch_release(
    client: &Client,
    repo: &str,
    requested: &str,
    archive_name: &str,
    checksum_name: &str,
) -> Result<Release> {
    if requested == "latest" {
        let endpoint = format!("https://api.github.com/repos/{repo}/releases?per_page=100");
        let releases: Vec<Release> = release_request(client, endpoint)
            .send()?
            .error_for_status()
            .with_context(|| format!("failed to list releases in {repo}"))?
            .json()
            .context("GitHub returned an invalid release list")?;
        return select_latest_release(releases, archive_name, checksum_name)
            .with_context(|| format!("no stable release in {repo} supports this platform"));
    }

    let endpoint = format!("https://api.github.com/repos/{repo}/releases/tags/{requested}");
    release_request(client, endpoint)
        .send()?
        .error_for_status()
        .with_context(|| format!("release {requested:?} was not found in {repo}"))?
        .json()
        .context("GitHub returned an invalid release response")
}

fn stable_version(tag: &str) -> Option<(u64, u64, u64)> {
    let mut parts = tag.strip_prefix('v')?.split('.');
    let version = (
        parts.next()?.parse().ok()?,
        parts.next()?.parse().ok()?,
        parts.next()?.parse().ok()?,
    );
    parts.next().is_none().then_some(version)
}

fn select_latest_release(
    releases: Vec<Release>,
    archive_name: &str,
    checksum_name: &str,
) -> Result<Release> {
    releases
        .into_iter()
        .filter(|release| !release.draft && !release.prerelease)
        .filter(|release| {
            find_asset(&release.assets, archive_name).is_ok()
                && find_asset(&release.assets, checksum_name).is_ok()
        })
        .filter_map(|release| stable_version(&release.tag_name).map(|version| (version, release)))
        .max_by_key(|(version, _)| *version)
        .map(|(_, release)| release)
        .context("release list contains no matching stable tool release")
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

    fn release(tag: &str, draft: bool, prerelease: bool, assets: &[&str]) -> Release {
        Release {
            tag_name: tag.to_owned(),
            draft,
            prerelease,
            assets: assets
                .iter()
                .map(|name| Asset {
                    name: (*name).to_owned(),
                    browser_download_url: format!("https://example.test/{name}"),
                })
                .collect(),
        }
    }

    #[test]
    fn validates_repository_names() {
        assert!(validate_repo("fabrica-eda/texo").is_ok());
        assert!(validate_repo("missing-owner").is_err());
        assert!(validate_repo("owner/repo/extra").is_err());
        assert!(validate_repo("owner/../repo").is_err());
    }

    #[test]
    fn parses_stable_release_versions() {
        assert_eq!(stable_version("v1.20.3"), Some((1, 20, 3)));
        for tag in ["1.20.3", "v1.20", "v1.20.3-rc.1", "txdb-ecp5-v3"] {
            assert_eq!(stable_version(tag), None, "accepted {tag:?}");
        }
    }

    #[test]
    fn parses_self_update_command() -> Result<()> {
        let cli = Cli::try_parse_from(["fabricaup", "self", "update"])?;
        assert!(matches!(
            cli.command,
            Command::Self_ {
                command: SelfCommand::Update
            }
        ));
        Ok(())
    }

    #[test]
    fn selects_highest_complete_stable_release() -> Result<()> {
        let archive = "texo-x86_64-unknown-linux-gnu.tar.gz";
        let checksum = "texo-x86_64-unknown-linux-gnu.sha256";
        let selected = select_latest_release(
            vec![
                release("txdb-ecp5-v3", false, false, &[archive, checksum]),
                release("v0.3.0", true, false, &[archive, checksum]),
                release("v0.2.0", false, false, &[archive]),
                release("v0.1.0", false, false, &[archive, checksum]),
                release("v0.1.1", false, false, &[archive, checksum]),
            ],
            archive,
            checksum,
        )?;
        assert_eq!(selected.tag_name, "v0.1.1");
        Ok(())
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
