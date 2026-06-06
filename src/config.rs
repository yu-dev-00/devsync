use anyhow::{bail, Context, Result};
use serde::Deserialize;
use std::{
    fs,
    path::{Path, PathBuf},
};

#[derive(Debug, Clone, Deserialize)]
pub struct Config {
    #[serde(default)]
    pub connection: ConnectionConfig,
    #[serde(default)]
    pub paths: PathConfig,
    #[serde(default)]
    pub commands: CommandConfig,
    #[serde(default)]
    pub sync: SyncConfig,
}

#[derive(Debug, Clone, Deserialize)]
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
pub struct CommandConfig {
    pub build: Option<String>,
    pub run: Option<String>,
    pub test: Option<String>,
}

#[derive(Debug, Clone, Default, Deserialize)]
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
        let raw = fs::read_to_string(path)
            .with_context(|| format!("failed to read {}", path.display()))?;
        let cfg: Config = toml::from_str(&raw)
            .with_context(|| format!("failed to parse {}", path.display()))?;
        cfg.validate()?;
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
        let value = match name {
            "build" => self.commands.build.as_deref(),
            "run" => self.commands.run.as_deref(),
            "test" => self.commands.test.as_deref(),
            other => bail!("unknown command name: {other}"),
        };
        value.ok_or_else(|| anyhow::anyhow!("commands.{name} is not defined"))
    }
}
