use anyhow::{bail, Context, Result};
use serde::Deserialize;
use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
};

pub const DEFAULT_CONFIG_PATH: &str = ".devsync/config.toml";

/// Prefer the private project configuration, retaining legacy projects as-is.
pub fn default_path() -> Result<PathBuf> {
    let preferred = PathBuf::from(DEFAULT_CONFIG_PATH);
    if preferred.try_exists()? || !Path::new("devsync.toml").try_exists()? {
        Ok(preferred)
    } else {
        Ok(PathBuf::from("devsync.toml"))
    }
}

/// The hidden config lives one directory below the source project root.
pub fn hidden_config_project_root(path: &Path) -> Option<&Path> {
    let parent = path.parent()?;
    if path.file_name()?.to_str()?.eq_ignore_ascii_case("config.toml")
        && parent.file_name()?.to_str()?.eq_ignore_ascii_case(".devsync") {
        Some(parent.parent().unwrap_or(Path::new(".")))
    } else {
        None
    }
}

// Unknown keys are errors: a misspelled `[path]` would otherwise surface as a
// missing `paths.remote_dir`, far from the typo.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    #[serde(default)]
    pub connection: ConnectionConfig,
    #[serde(default)]
    pub paths: PathConfig,
    #[serde(default)]
    pub commands: BTreeMap<String, String>,
    #[serde(default)]
    pub sync: SyncConfig,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ConnectionConfig {
    #[serde(default)]
    pub host: String,
    #[serde(default)]
    pub user: String,
    #[serde(default = "default_port")]
    pub port: u16,
    #[serde(default = "default_agent_path")]
    pub agent_path: String,
}

impl Default for ConnectionConfig {
    fn default() -> Self {
        Self {
            host: String::new(),
            user: String::new(),
            port: default_port(),
            agent_path: default_agent_path(),
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PathConfig {
    #[serde(default = "default_local_dir")]
    pub local_dir: PathBuf,
    #[serde(default)]
    pub remote_dir: String,
}

impl Default for PathConfig {
    fn default() -> Self {
        Self {
            local_dir: default_local_dir(),
            remote_dir: String::new(),
        }
    }
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SyncConfig {
    #[serde(default)]
    pub exclude: Vec<String>,
}

fn default_port() -> u16 {
    22
}
fn default_agent_path() -> String {
    "devsync.exe".to_string()
}
fn default_local_dir() -> PathBuf {
    PathBuf::from(".")
}

impl Config {
    pub fn load(path: &Path) -> Result<Self> {
        if !path.try_exists()? {
            bail!(
                "no devsync config at {} — run `devsync init`, run from the project root, or pass --config <path>",
                path.display()
            );
        }
        let raw = fs::read_to_string(path)
            .with_context(|| format!("failed to read {}", path.display()))?;
        let mut cfg: Config = toml::from_str(&raw)
            .with_context(|| format!("failed to parse {}", path.display()))?;
        if let Err(error) = cfg.validate() {
            bail!("{}: {error}", path.display());
        }
        if cfg.paths.local_dir.is_relative() {
            if let Some(root) = hidden_config_project_root(path) {
                cfg.paths.local_dir = root.join(&cfg.paths.local_dir);
            }
        }
        Ok(cfg)
    }

    pub fn validate(&self) -> Result<()> {
        let mut missing = Vec::new();
        if self.connection.host.trim().is_empty() {
            missing.push("connection.host");
        }
        if self.connection.user.trim().is_empty() {
            missing.push("connection.user");
        }
        if self.paths.remote_dir.trim().is_empty() {
            missing.push("paths.remote_dir");
        }
        if !missing.is_empty() {
            bail!("missing required config fields: {}", missing.join(", "));
        }
        Ok(())
    }

    pub fn command(&self, name: &str) -> Result<&str> {
        self.commands
            .get(name)
            .map(String::as_str)
            .ok_or_else(|| {
                let defined: Vec<&str> = self.commands.keys().map(String::as_str).collect();
                if defined.is_empty() {
                    anyhow::anyhow!("commands.{name} is not defined; [commands] is empty")
                } else {
                    // Real projects define dozens; a typo needs a hint, not the whole list.
                    const SHOWN: usize = 10;
                    let more = defined.len().saturating_sub(SHOWN);
                    let suffix = if more > 0 { format!(", … ({more} more in [commands])") } else { String::new() };
                    anyhow::anyhow!("commands.{name} is not defined; defined: {}{suffix}", defined[..defined.len().min(SHOWN)].join(", "))
                }
            })
    }
}
