use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::ffi::OsStr;
use std::fs;
use std::path::{Component, Path, PathBuf};

pub const DEFAULT_ORG: &str = "fabrica-eda";
pub const DEFAULT_TOOL: &str = "texo";

#[derive(Debug, Clone)]
pub struct Layout {
    pub home: PathBuf,
    pub bin: PathBuf,
    pub toolchains: PathBuf,
    active_file: PathBuf,
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct ActiveTools {
    #[serde(default)]
    tools: BTreeMap<String, ActiveTool>,
}

#[derive(Debug, Serialize, Deserialize)]
struct ActiveTool {
    version: String,
    files: Vec<String>,
}

impl Layout {
    pub fn discover() -> Result<Self> {
        let home = match std::env::var_os("FABRICAUP_HOME") {
            Some(path) => PathBuf::from(path),
            None => dirs::home_dir()
                .context("could not determine the home directory")?
                .join(".fabrica"),
        };
        Ok(Self::new(home))
    }

    pub fn new(home: PathBuf) -> Self {
        Self {
            bin: home.join("bin"),
            toolchains: home.join("toolchains"),
            active_file: home.join("active.json"),
            home,
        }
    }

    pub fn initialize(&self) -> Result<()> {
        fs::create_dir_all(&self.bin)
            .with_context(|| format!("failed to create {}", self.bin.display()))?;
        fs::create_dir_all(&self.toolchains)
            .with_context(|| format!("failed to create {}", self.toolchains.display()))?;
        Ok(())
    }

    pub fn toolchain_dir(&self, tool: &str, version: &str) -> Result<PathBuf> {
        validate_tool(tool)?;
        validate_version(version)?;
        Ok(self.toolchains.join(tool).join(version))
    }

    pub fn installed(&self, tool: &str) -> Result<Vec<String>> {
        validate_tool(tool)?;
        let root = self.toolchains.join(tool);
        if !root.exists() {
            return Ok(Vec::new());
        }
        let mut versions = Vec::new();
        for entry in fs::read_dir(root)? {
            let entry = entry?;
            if entry.file_type()?.is_dir() {
                let name = entry.file_name().to_string_lossy().into_owned();
                if !name.starts_with('.') {
                    versions.push(name);
                }
            }
        }
        versions.sort();
        Ok(versions)
    }

    pub fn default_version(&self, tool: &str) -> Result<Option<String>> {
        validate_tool(tool)?;
        Ok(self
            .read_active()?
            .tools
            .get(tool)
            .map(|active| active.version.clone()))
    }

    pub fn activate(&self, tool: &str, version: &str) -> Result<Vec<String>> {
        let source = self.toolchain_dir(tool, version)?.join("bin");
        if !source.is_dir() {
            bail!("{tool} toolchain {version} is not installed");
        }
        self.initialize()?;

        let mut binaries = Vec::new();
        for entry in fs::read_dir(&source)? {
            let entry = entry?;
            if entry.file_type()?.is_file() && !is_manager_binary(&entry.file_name()) {
                binaries.push((
                    entry.file_name().to_string_lossy().into_owned(),
                    entry.path(),
                ));
            }
        }
        if binaries.is_empty() {
            bail!("{tool} toolchain {version} contains no binaries");
        }

        let mut active = self.read_active()?;
        for (name, _) in &binaries {
            if let Some((owner, _)) = active
                .tools
                .iter()
                .find(|(owner, entry)| owner.as_str() != tool && entry.files.contains(name))
            {
                bail!("cannot activate {tool}: binary {name} is already managed by {owner}");
            }
        }

        let previous = active
            .tools
            .remove(tool)
            .map(|entry| entry.files)
            .unwrap_or_default();
        for name in previous {
            let path = self.bin.join(name);
            if path.is_file() {
                fs::remove_file(path)?;
            }
        }

        let mut installed = Vec::new();
        for (name, path) in binaries {
            fs::copy(&path, self.bin.join(&name))
                .with_context(|| format!("failed to activate {}", path.display()))?;
            installed.push(name);
        }

        active.tools.insert(
            tool.to_owned(),
            ActiveTool {
                version: version.to_owned(),
                files: installed.clone(),
            },
        );
        self.write_active(&active)?;
        Ok(installed)
    }

    pub fn remove(&self, tool: &str, version: &str) -> Result<()> {
        if self.default_version(tool)?.as_deref() == Some(version) {
            bail!("cannot uninstall the active {tool} toolchain; select another default first");
        }
        let path = self.toolchain_dir(tool, version)?;
        if !path.exists() {
            bail!("{tool} toolchain {version} is not installed");
        }
        fs::remove_dir_all(path)?;
        Ok(())
    }

    fn read_active(&self) -> Result<ActiveTools> {
        match fs::read(&self.active_file) {
            Ok(bytes) => serde_json::from_slice(&bytes).context("failed to parse active.json"),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                Ok(ActiveTools::default())
            }
            Err(error) => Err(error).context("failed to read active.json"),
        }
    }

    fn write_active(&self, active: &ActiveTools) -> Result<()> {
        fs::write(&self.active_file, serde_json::to_vec_pretty(active)?)
            .context("failed to write active.json")
    }
}

pub fn validate_version(version: &str) -> Result<()> {
    let path = Path::new(version);
    if version.is_empty()
        || version == "."
        || version == ".."
        || path.components().count() != 1
        || !matches!(path.components().next(), Some(Component::Normal(_)))
        || version.contains(['/', '\\'])
    {
        bail!("invalid version: {version}");
    }
    Ok(())
}

pub fn validate_tool(tool: &str) -> Result<()> {
    if validate_version(tool).is_err()
        || !tool
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_' | '.'))
    {
        bail!("invalid tool name: {tool}");
    }
    Ok(())
}

pub fn target_triple() -> Result<&'static str> {
    match (std::env::consts::OS, std::env::consts::ARCH) {
        ("linux", "x86_64") => Ok("x86_64-unknown-linux-gnu"),
        ("linux", "aarch64") => Ok("aarch64-unknown-linux-gnu"),
        ("macos", "x86_64") => Ok("x86_64-apple-darwin"),
        ("macos", "aarch64") => Ok("aarch64-apple-darwin"),
        ("windows", "x86_64") => Ok("x86_64-pc-windows-msvc"),
        (os, arch) => bail!("unsupported platform: {os}/{arch}"),
    }
}

fn is_manager_binary(name: &OsStr) -> bool {
    matches!(name.to_str(), Some("fabricaup" | "fabricaup.exe"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_unsafe_versions() {
        for version in ["", ".", "..", "../oops", "a/b", "a\\b"] {
            assert!(validate_version(version).is_err(), "accepted {version:?}");
        }
        assert!(validate_version("v1.2.3-rc.1").is_ok());
    }

    #[test]
    fn rejects_unsafe_tool_names() {
        for tool in ["", "..", "../texo", "tool/name", "tool name", "tool!"] {
            assert!(validate_tool(tool).is_err(), "accepted {tool:?}");
        }
        assert!(validate_tool("texo-next").is_ok());
    }

    #[test]
    fn activates_and_switches_toolchains() -> Result<()> {
        let temp = tempfile::tempdir()?;
        let layout = Layout::new(temp.path().join("home"));
        for (tool, version) in [("texo", "v1"), ("texo", "v2"), ("struo", "v1")] {
            let bin = layout.toolchain_dir(tool, version)?.join("bin");
            fs::create_dir_all(&bin)?;
            fs::write(bin.join(tool), version)?;
        }

        layout.activate("texo", "v1")?;
        assert!(layout.bin.join("texo").exists());
        layout.activate("struo", "v1")?;
        layout.activate("texo", "v2")?;
        assert!(layout.bin.join("texo").exists());
        assert!(layout.bin.join("struo").exists());
        assert_eq!(fs::read_to_string(layout.bin.join("texo"))?, "v2");
        assert_eq!(layout.default_version("texo")?.as_deref(), Some("v2"));
        assert_eq!(layout.default_version("struo")?.as_deref(), Some("v1"));
        Ok(())
    }
}
