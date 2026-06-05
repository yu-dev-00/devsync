# devsync Agent Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Build the first Windows-focused `devsync` CLI that syncs local files to a remote Windows copy through an SSH-launched stdio agent and runs named remote commands.

**Architecture:** One Rust executable has a local CLI role and a remote `agent --stdio` role. The local role reads `devsync.toml`, starts `ssh.exe`, speaks a length-prefixed JSON protocol to the remote agent, compares BLAKE3 manifests, uploads changed files, optionally deletes remote-only files, and requests `build`, `run`, or `test`.

**Tech Stack:** Rust 2021, `clap`, `serde`, `toml`, `serde_json`, `blake3`, `walkdir`, `anyhow`, `thiserror`, `tempfile`, Windows OpenSSH `ssh.exe`.

---

## File Structure

This workspace is currently not a git repository. Keep the commit steps in the task flow for the intended development branch. If execution starts before `git init` or a worktree is created, run the implementation and verification steps and record that the commit step was skipped because no `.git` directory exists.

Create:

- `Cargo.toml` - package metadata, dependencies, binary target.
- `devsync.toml.example` - example user configuration.
- `src/main.rs` - CLI parsing and top-level command dispatch.
- `src/config.rs` - `devsync.toml` structures, defaults, validation, command lookup.
- `src/path_safety.rs` - slash-normalized relative paths and remote write safety checks.
- `src/exclude.rs` - forced excludes and configured exclude matching.
- `src/manifest.rs` - directory walking, BLAKE3 hashing, manifest structures.
- `src/diff.rs` - upload/delete/skip plan calculation.
- `src/protocol.rs` - message enum and frame encode/decode.
- `src/agent.rs` - stdio agent loop, remote manifest, file apply, exec handling.
- `src/client.rs` - local SSH child process startup and protocol client.
- `src/sync.rs` - high-level status/sync/build/test/run flows.
- `tests/config_tests.rs` - config behavior.
- `tests/manifest_diff_tests.rs` - exclude, manifest, diff, and path safety behavior.
- `tests/protocol_tests.rs` - frame and message behavior.
- `tests/agent_stdio_tests.rs` - child-process agent protocol integration without SSH.

Do not create SFTP, TCP server, daemon, logs, clean, shell, Git object storage, or bidirectional sync modules in the first implementation.

## Task 1: Scaffold Rust Project and CLI

**Files:**
- Create: `Cargo.toml`
- Create: `src/main.rs`
- Create: `devsync.toml.example`

- [ ] **Step 1: Write the initial package manifest**

Create `Cargo.toml`:

```toml
[package]
name = "devsync"
version = "0.1.0"
edition = "2021"

[dependencies]
anyhow = "1"
blake3 = "1"
clap = { version = "4", features = ["derive"] }
serde = { version = "1", features = ["derive"] }
serde_json = "1"
thiserror = "1"
toml = "0.8"
walkdir = "2"

[dev-dependencies]
assert_cmd = "2"
predicates = "3"
tempfile = "3"
```

- [ ] **Step 2: Write the initial CLI entry point**

Create `src/main.rs`:

```rust
use anyhow::Result;
use clap::{Args, Parser, Subcommand};
use std::path::PathBuf;

#[derive(Debug, Parser)]
#[command(name = "devsync")]
#[command(about = "Sync local projects to a remote Windows execution copy")]
struct Cli {
    #[arg(long, default_value = "devsync.toml")]
    config: PathBuf,

    #[arg(short, long)]
    verbose: bool,

    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    Status,
    Sync(SyncArgs),
    Build,
    Run,
    Test,
    Agent(AgentArgs),
}

#[derive(Debug, Args)]
struct SyncArgs {
    #[arg(long)]
    delete: bool,
}

#[derive(Debug, Args)]
struct AgentArgs {
    #[arg(long)]
    stdio: bool,
}

fn main() -> Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Command::Status => {
            println!("status is not implemented yet");
        }
        Command::Sync(args) => {
            println!("sync is not implemented yet; delete={}", args.delete);
        }
        Command::Build => {
            println!("build is not implemented yet");
        }
        Command::Run => {
            println!("run is not implemented yet");
        }
        Command::Test => {
            println!("test is not implemented yet");
        }
        Command::Agent(args) => {
            if !args.stdio {
                anyhow::bail!("agent requires --stdio");
            }
            println!("agent stdio is not implemented yet");
        }
    }

    Ok(())
}
```

- [ ] **Step 3: Add example config**

Create `devsync.toml.example`:

```toml
[connection]
host = "remote-pc"
user = "user"
port = 22
agent_path = "C:\\tools\\devsync.exe"

[paths]
local_dir = "."
remote_dir = "C:\\work\\project"

[commands]
build = "powershell -NoProfile -ExecutionPolicy Bypass -File .\\build.ps1"
run = "powershell -NoProfile -ExecutionPolicy Bypass -File .\\run.ps1"
test = "powershell -NoProfile -ExecutionPolicy Bypass -File .\\test.ps1"

[sync]
exclude = [
  ".git",
  ".devsync",
  "devsync.toml",
  "bin",
  "obj",
  "build",
  "dist",
  "logs",
  "artifacts",
  "node_modules",
  ".vs",
  ".vscode"
]
```

- [ ] **Step 4: Verify CLI compiles**

Run: `cargo test`

Expected: PASS with no tests or only generated harnesses.

- [ ] **Step 5: Commit**

```bash
git add Cargo.toml src/main.rs devsync.toml.example
git commit -m "feat: scaffold devsync cli"
```

## Task 2: Config Parsing and Validation

**Files:**
- Create: `src/config.rs`
- Modify: `src/main.rs`
- Create: `tests/config_tests.rs`

- [ ] **Step 1: Write failing config tests**

Create `tests/config_tests.rs`:

```rust
use std::fs;

#[path = "../src/config.rs"]
mod config;

#[test]
fn loads_defaults_and_required_fields() {
    let dir = tempfile::tempdir().unwrap();
    let config_path = dir.path().join("devsync.toml");
    fs::write(
        &config_path,
        r#"
[connection]
host = "remote-pc"
user = "alice"

[paths]
remote_dir = "C:\\work\\project"
"#,
    )
    .unwrap();

    let cfg = config::Config::load(&config_path).unwrap();

    assert_eq!(cfg.connection.host, "remote-pc");
    assert_eq!(cfg.connection.user, "alice");
    assert_eq!(cfg.connection.port, 22);
    assert_eq!(cfg.connection.agent_path, "devsync.exe");
    assert_eq!(cfg.paths.local_dir.to_string_lossy(), ".");
    assert_eq!(cfg.paths.remote_dir, r"C:\work\project");
}

#[test]
fn rejects_missing_required_fields() {
    let dir = tempfile::tempdir().unwrap();
    let config_path = dir.path().join("devsync.toml");
    fs::write(&config_path, "[connection]\nhost = \"remote-pc\"\n").unwrap();

    let error = config::Config::load(&config_path).unwrap_err().to_string();

    assert!(error.contains("connection.user"));
    assert!(error.contains("paths.remote_dir"));
}

#[test]
fn command_is_required_only_when_requested() {
    let cfg = config::Config {
        connection: config::ConnectionConfig {
            host: "remote-pc".to_string(),
            user: "alice".to_string(),
            port: 22,
            agent_path: "devsync.exe".to_string(),
        },
        paths: config::PathConfig {
            local_dir: ".".into(),
            remote_dir: r"C:\work\project".to_string(),
        },
        commands: config::CommandConfig::default(),
        sync: config::SyncConfig::default(),
    };

    assert!(cfg.command("build").unwrap_err().to_string().contains("commands.build"));
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test --test config_tests`

Expected: FAIL because `src/config.rs` does not exist or `Config` is not defined.

- [ ] **Step 3: Implement config module**

Create `src/config.rs`:

```rust
use anyhow::{bail, Context, Result};
use serde::Deserialize;
use std::{fs, path::{Path, PathBuf}};

#[derive(Debug, Clone, Deserialize)]
pub struct Config {
    pub connection: ConnectionConfig,
    pub paths: PathConfig,
    #[serde(default)]
    pub commands: CommandConfig,
    #[serde(default)]
    pub sync: SyncConfig,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ConnectionConfig {
    pub host: String,
    pub user: String,
    #[serde(default = "default_port")]
    pub port: u16,
    #[serde(default = "default_agent_path")]
    pub agent_path: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct PathConfig {
    #[serde(default = "default_local_dir")]
    pub local_dir: PathBuf,
    pub remote_dir: String,
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

fn default_port() -> u16 { 22 }
fn default_agent_path() -> String { "devsync.exe".to_string() }
fn default_local_dir() -> PathBuf { PathBuf::from(".") }

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
```

- [ ] **Step 4: Wire config module into main**

Modify the top of `src/main.rs`:

```rust
mod config;
```

In non-agent commands, load the config before printing the placeholder:

```rust
let cfg = if matches!(&cli.command, Command::Agent(_)) {
    None
} else {
    Some(config::Config::load(&cli.config)?)
};
```

Then bind `let _cfg = cfg;` before the `match` until later tasks use it.

- [ ] **Step 5: Run tests**

Run: `cargo test --test config_tests`

Expected: PASS.

- [ ] **Step 6: Commit**

```bash
git add src/config.rs src/main.rs tests/config_tests.rs
git commit -m "feat: load devsync config"
```

## Task 3: Path Safety, Excludes, Manifest, and Diff

**Files:**
- Create: `src/path_safety.rs`
- Create: `src/exclude.rs`
- Create: `src/manifest.rs`
- Create: `src/diff.rs`
- Modify: `src/main.rs`
- Create: `tests/manifest_diff_tests.rs`

- [ ] **Step 1: Write failing tests**

Create `tests/manifest_diff_tests.rs`:

```rust
use std::fs;

#[path = "../src/path_safety.rs"]
mod path_safety;
#[path = "../src/exclude.rs"]
mod exclude;
#[path = "../src/manifest.rs"]
mod manifest;
#[path = "../src/diff.rs"]
mod diff;

#[test]
fn rejects_unsafe_relative_paths() {
    assert!(path_safety::validate_relative_path("src/main.rs").is_ok());
    assert!(path_safety::validate_relative_path("../secret.txt").is_err());
    assert!(path_safety::validate_relative_path("src/../../secret.txt").is_err());
    assert!(path_safety::validate_relative_path("C:/secret.txt").is_err());
    assert!(path_safety::validate_relative_path("/secret.txt").is_err());
}

#[test]
fn forced_excludes_always_apply() {
    let matcher = exclude::ExcludeMatcher::new(vec![]).unwrap();
    assert!(matcher.is_excluded(".git/config"));
    assert!(matcher.is_excluded(".devsync/state"));
    assert!(matcher.is_excluded("devsync.toml"));
    assert!(!matcher.is_excluded("src/main.rs"));
}

#[test]
fn manifest_uses_slash_paths_and_hashes_content() {
    let dir = tempfile::tempdir().unwrap();
    fs::create_dir_all(dir.path().join("src")).unwrap();
    fs::write(dir.path().join("src").join("main.txt"), "hello").unwrap();
    fs::write(dir.path().join("devsync.toml"), "secret").unwrap();

    let matcher = exclude::ExcludeMatcher::new(vec![]).unwrap();
    let manifest = manifest::build_manifest(dir.path(), &matcher).unwrap();

    assert_eq!(manifest.files.len(), 1);
    assert_eq!(manifest.files[0].path, "src/main.txt");
    assert_eq!(manifest.files[0].size, 5);
    assert_eq!(manifest.files[0].hash, blake3::hash(b"hello").to_hex().to_string());
}

#[test]
fn diff_identifies_uploads_deletes_and_skips() {
    let local = manifest::Manifest {
        files: vec![
            manifest::ManifestEntry { path: "a.txt".into(), size: 1, hash: "h1".into() },
            manifest::ManifestEntry { path: "b.txt".into(), size: 2, hash: "h2-new".into() },
        ],
    };
    let remote = manifest::Manifest {
        files: vec![
            manifest::ManifestEntry { path: "b.txt".into(), size: 2, hash: "h2-old".into() },
            manifest::ManifestEntry { path: "c.txt".into(), size: 3, hash: "h3".into() },
        ],
    };

    let plan = diff::calculate_diff(&local, &remote, true);

    assert_eq!(plan.upload, vec!["a.txt", "b.txt"]);
    assert_eq!(plan.delete, vec!["c.txt"]);
    assert_eq!(plan.skipped, 0);
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test --test manifest_diff_tests`

Expected: FAIL because modules are not implemented.

- [ ] **Step 3: Implement path safety**

Create `src/path_safety.rs`:

```rust
use anyhow::{bail, Result};
use std::path::{Component, Path};

pub fn normalize_relative_path(path: &Path) -> Result<String> {
    let mut parts = Vec::new();
    for component in path.components() {
        match component {
            Component::Normal(value) => parts.push(value.to_string_lossy().to_string()),
            Component::CurDir => {}
            _ => bail!("unsafe path component in {}", path.display()),
        }
    }
    let normalized = parts.join("/");
    validate_relative_path(&normalized)?;
    Ok(normalized)
}

pub fn validate_relative_path(path: &str) -> Result<()> {
    if path.trim().is_empty() {
        bail!("empty path is not allowed");
    }
    let normalized = path.replace('\\', "/");
    if normalized.starts_with('/') || normalized.contains(':') {
        bail!("absolute path is not allowed: {path}");
    }
    for part in normalized.split('/') {
        if part.is_empty() || part == "." || part == ".." {
            bail!("unsafe relative path: {path}");
        }
    }
    Ok(())
}
```

- [ ] **Step 4: Implement excludes**

Create `src/exclude.rs`:

```rust
use anyhow::Result;

const FORCED_EXCLUDES: &[&str] = &["devsync.toml", ".devsync", ".git"];

#[derive(Debug, Clone)]
pub struct ExcludeMatcher {
    patterns: Vec<String>,
}

impl ExcludeMatcher {
    pub fn new(patterns: Vec<String>) -> Result<Self> {
        Ok(Self { patterns })
    }

    pub fn is_excluded(&self, path: &str) -> bool {
        let path = path.replace('\\', "/");
        FORCED_EXCLUDES.iter().any(|pattern| matches_pattern(&path, pattern))
            || self.patterns.iter().any(|pattern| matches_pattern(&path, pattern))
    }
}

fn matches_pattern(path: &str, pattern: &str) -> bool {
    let pattern = pattern.trim_matches('/').replace('\\', "/");
    path == pattern || path.starts_with(&format!("{pattern}/")) || path.split('/').any(|part| part == pattern)
}
```

- [ ] **Step 5: Implement manifest builder**

Create `src/manifest.rs`:

```rust
use crate::{exclude::ExcludeMatcher, path_safety};
use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::{fs, path::Path};
use walkdir::WalkDir;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Manifest {
    pub files: Vec<ManifestEntry>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ManifestEntry {
    pub path: String,
    pub size: u64,
    pub hash: String,
}

pub fn build_manifest(root: &Path, excludes: &ExcludeMatcher) -> Result<Manifest> {
    let mut files = Vec::new();
    for entry in WalkDir::new(root).into_iter().filter_map(Result::ok) {
        if !entry.file_type().is_file() {
            continue;
        }
        let relative_path = entry.path().strip_prefix(root)?;
        let normalized = path_safety::normalize_relative_path(relative_path)?;
        if excludes.is_excluded(&normalized) {
            continue;
        }
        let bytes = fs::read(entry.path())?;
        files.push(ManifestEntry {
            path: normalized,
            size: bytes.len() as u64,
            hash: blake3::hash(&bytes).to_hex().to_string(),
        });
    }
    files.sort_by(|a, b| a.path.cmp(&b.path));
    Ok(Manifest { files })
}
```

- [ ] **Step 6: Implement diff calculation**

Create `src/diff.rs`:

```rust
use crate::manifest::Manifest;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SyncPlan {
    pub upload: Vec<String>,
    pub delete: Vec<String>,
    pub skipped: usize,
}

pub fn calculate_diff(local: &Manifest, remote: &Manifest, include_delete: bool) -> SyncPlan {
    let local_by_path: BTreeMap<_, _> = local.files.iter().map(|entry| (&entry.path, entry)).collect();
    let remote_by_path: BTreeMap<_, _> = remote.files.iter().map(|entry| (&entry.path, entry)).collect();

    let mut upload = Vec::new();
    let mut skipped = 0;
    for local_entry in &local.files {
        match remote_by_path.get(&local_entry.path) {
            Some(remote_entry) if remote_entry.size == local_entry.size && remote_entry.hash == local_entry.hash => {
                skipped += 1;
            }
            _ => upload.push(local_entry.path.clone()),
        }
    }

    let delete = if include_delete {
        remote
            .files
            .iter()
            .filter(|entry| !local_by_path.contains_key(&entry.path))
            .map(|entry| entry.path.clone())
            .collect()
    } else {
        Vec::new()
    };

    SyncPlan { upload, delete, skipped }
}
```

- [ ] **Step 7: Register modules**

Modify `src/main.rs` top:

```rust
mod config;
mod diff;
mod exclude;
mod manifest;
mod path_safety;
```

- [ ] **Step 8: Run tests**

Run: `cargo test --test manifest_diff_tests`

Expected: PASS.

- [ ] **Step 9: Commit**

```bash
git add src/path_safety.rs src/exclude.rs src/manifest.rs src/diff.rs src/main.rs tests/manifest_diff_tests.rs
git commit -m "feat: add manifest diff engine"
```

## Task 4: Protocol Frames and Messages

**Files:**
- Create: `src/protocol.rs`
- Modify: `src/main.rs`
- Create: `tests/protocol_tests.rs`

- [ ] **Step 1: Write failing protocol tests**

Create `tests/protocol_tests.rs`:

```rust
use std::io::Cursor;

#[path = "../src/manifest.rs"]
mod manifest;
#[path = "../src/path_safety.rs"]
mod path_safety;
#[path = "../src/exclude.rs"]
mod exclude;
#[path = "../src/diff.rs"]
mod diff;
#[path = "../src/protocol.rs"]
mod protocol;

#[test]
fn round_trips_json_frame() {
    let message = protocol::Message::Hello { version: 1 };
    let mut bytes = Vec::new();

    protocol::write_message(&mut bytes, &message).unwrap();
    let decoded = protocol::read_message(&mut Cursor::new(bytes)).unwrap();

    assert_eq!(decoded, message);
}

#[test]
fn file_message_declares_payload_size() {
    let message = protocol::Message::File {
        path: "src/main.txt".into(),
        size: 5,
        hash: "abc".into(),
    };

    let json = serde_json::to_string(&message).unwrap();

    assert!(json.contains("\"type\":\"file\""));
    assert!(json.contains("\"size\":5"));
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test --test protocol_tests`

Expected: FAIL because `src/protocol.rs` is missing.

- [ ] **Step 3: Implement protocol module**

Create `src/protocol.rs`:

```rust
use crate::{diff::SyncPlan, manifest::Manifest};
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::io::{Read, Write};

pub const PROTOCOL_VERSION: u32 = 1;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Message {
    Hello { version: u32 },
    Config { remote_dir: String, commands: BTreeMap<String, String> },
    ManifestRequest,
    Manifest { files: Vec<crate::manifest::ManifestEntry> },
    SyncPlan { upload: Vec<String>, delete: Vec<String> },
    File { path: String, size: u64, hash: String },
    SyncComplete { uploaded: usize, deleted: usize, skipped: usize },
    Exec { name: String },
    Output { stream: String, data: String },
    Exit { code: i32 },
    Error { message: String },
}

impl From<Manifest> for Message {
    fn from(value: Manifest) -> Self {
        Message::Manifest { files: value.files }
    }
}

impl From<SyncPlan> for Message {
    fn from(value: SyncPlan) -> Self {
        Message::SyncPlan { upload: value.upload, delete: value.delete }
    }
}

pub fn write_message<W: Write>(writer: &mut W, message: &Message) -> Result<()> {
    let json = serde_json::to_vec(message)?;
    let len = u32::try_from(json.len()).context("message is too large")?;
    writer.write_all(&len.to_be_bytes())?;
    writer.write_all(&json)?;
    writer.flush()?;
    Ok(())
}

pub fn read_message<R: Read>(reader: &mut R) -> Result<Message> {
    let mut len_buf = [0u8; 4];
    reader.read_exact(&mut len_buf)?;
    let len = u32::from_be_bytes(len_buf) as usize;
    let mut json = vec![0u8; len];
    reader.read_exact(&mut json)?;
    Ok(serde_json::from_slice(&json)?)
}
```

- [ ] **Step 4: Register module**

Modify `src/main.rs` top:

```rust
mod protocol;
```

- [ ] **Step 5: Run protocol tests**

Run: `cargo test --test protocol_tests`

Expected: PASS.

- [ ] **Step 6: Commit**

```bash
git add src/protocol.rs src/main.rs tests/protocol_tests.rs
git commit -m "feat: add stdio protocol frames"
```

## Task 5: Agent Stdio Loop

**Files:**
- Create: `src/agent.rs`
- Modify: `src/main.rs`
- Create: `tests/agent_stdio_tests.rs`

- [ ] **Step 1: Write failing agent unit-style integration test**

Create `tests/agent_stdio_tests.rs`:

```rust
use std::collections::BTreeMap;
use std::io::Cursor;

#[path = "../src/path_safety.rs"]
mod path_safety;
#[path = "../src/exclude.rs"]
mod exclude;
#[path = "../src/manifest.rs"]
mod manifest;
#[path = "../src/diff.rs"]
mod diff;
#[path = "../src/protocol.rs"]
mod protocol;
#[path = "../src/agent.rs"]
mod agent;

#[test]
fn agent_rejects_manifest_before_config() {
    let mut input = Vec::new();
    protocol::write_message(&mut input, &protocol::Message::ManifestRequest).unwrap();

    let mut output = Vec::new();
    agent::run_agent(Cursor::new(input), &mut output).unwrap();

    let response = protocol::read_message(&mut Cursor::new(output)).unwrap();
    assert_eq!(
        response,
        protocol::Message::Error { message: "agent config has not been received".into() }
    );
}

#[test]
fn agent_accepts_config_and_returns_manifest() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("remote.txt"), "remote").unwrap();

    let mut commands = BTreeMap::new();
    commands.insert("run".to_string(), "echo ok".to_string());

    let mut input = Vec::new();
    protocol::write_message(
        &mut input,
        &protocol::Message::Config {
            remote_dir: dir.path().to_string_lossy().to_string(),
            commands,
        },
    )
    .unwrap();
    protocol::write_message(&mut input, &protocol::Message::ManifestRequest).unwrap();

    let mut output = Vec::new();
    agent::run_agent(Cursor::new(input), &mut output).unwrap();

    let response = protocol::read_message(&mut Cursor::new(output)).unwrap();
    match response {
        protocol::Message::Manifest { files } => {
            assert_eq!(files.len(), 1);
            assert_eq!(files[0].path, "remote.txt");
        }
        other => panic!("unexpected response: {other:?}"),
    }
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test --test agent_stdio_tests`

Expected: FAIL because `src/agent.rs` is missing.

- [ ] **Step 3: Implement initial agent loop**

Create `src/agent.rs`:

```rust
use crate::{exclude::ExcludeMatcher, manifest, protocol::{self, Message}};
use anyhow::Result;
use std::collections::BTreeMap;
use std::io::{Read, Write};
use std::path::PathBuf;

#[derive(Debug, Clone)]
struct AgentConfig {
    remote_dir: PathBuf,
    commands: BTreeMap<String, String>,
}

pub fn run_stdio_agent() -> Result<()> {
    let stdin = std::io::stdin();
    let stdout = std::io::stdout();
    run_agent(stdin.lock(), stdout.lock())
}

pub fn run_agent<R: Read, W: Write>(mut reader: R, mut writer: W) -> Result<()> {
    let mut config: Option<AgentConfig> = None;

    loop {
        let message = match protocol::read_message(&mut reader) {
            Ok(message) => message,
            Err(_) => break,
        };

        match message {
            Message::Config { remote_dir, commands } => {
                config = Some(AgentConfig { remote_dir: PathBuf::from(remote_dir), commands });
            }
            Message::ManifestRequest => {
                let Some(config) = &config else {
                    protocol::write_message(
                        &mut writer,
                        &Message::Error { message: "agent config has not been received".into() },
                    )?;
                    continue;
                };
                std::fs::create_dir_all(&config.remote_dir)?;
                let matcher = ExcludeMatcher::new(Vec::new())?;
                let manifest = manifest::build_manifest(&config.remote_dir, &matcher)?;
                protocol::write_message(&mut writer, &manifest.into())?;
            }
            Message::Hello { version } => {
                protocol::write_message(&mut writer, &Message::Hello { version })?;
            }
            _ => {
                protocol::write_message(
                    &mut writer,
                    &Message::Error { message: "unsupported agent message".into() },
                )?;
            }
        }
    }

    Ok(())
}
```

- [ ] **Step 4: Wire `agent --stdio`**

Modify `src/main.rs`:

```rust
mod agent;
```

Replace the `Command::Agent(args)` branch:

```rust
Command::Agent(args) => {
    if !args.stdio {
        anyhow::bail!("agent requires --stdio");
    }
    agent::run_stdio_agent()?;
}
```

- [ ] **Step 5: Run agent tests**

Run: `cargo test --test agent_stdio_tests`

Expected: PASS.

- [ ] **Step 6: Commit**

```bash
git add src/agent.rs src/main.rs tests/agent_stdio_tests.rs
git commit -m "feat: add stdio agent manifest handling"
```

## Task 6: Local Client and Status Flow

**Files:**
- Create: `src/client.rs`
- Create: `src/sync.rs`
- Modify: `src/main.rs`

- [ ] **Step 1: Implement SSH client wrapper**

Create `src/client.rs`:

```rust
use crate::{config::Config, protocol::{self, Message}};
use anyhow::{Context, Result};
use std::collections::BTreeMap;
use std::io::{BufReader, BufWriter};
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};

pub struct RemoteClient {
    child: Child,
    reader: BufReader<ChildStdout>,
    writer: BufWriter<ChildStdin>,
}

impl RemoteClient {
    pub fn connect(config: &Config) -> Result<Self> {
        let target = format!("{}@{}", config.connection.user, config.connection.host);
        let remote_command = format!("\"{}\" agent --stdio", config.connection.agent_path);
        let mut child = Command::new("ssh")
            .arg("-p")
            .arg(config.connection.port.to_string())
            .arg(target)
            .arg(remote_command)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn()
            .context("failed to start ssh.exe")?;

        let stdin = child.stdin.take().context("failed to open ssh stdin")?;
        let stdout = child.stdout.take().context("failed to open ssh stdout")?;

        let mut client = Self {
            child,
            reader: BufReader::new(stdout),
            writer: BufWriter::new(stdin),
        };
        client.send_config(config)?;
        Ok(client)
    }

    fn send_config(&mut self, config: &Config) -> Result<()> {
        let mut commands = BTreeMap::new();
        if let Some(value) = &config.commands.build {
            commands.insert("build".to_string(), value.clone());
        }
        if let Some(value) = &config.commands.run {
            commands.insert("run".to_string(), value.clone());
        }
        if let Some(value) = &config.commands.test {
            commands.insert("test".to_string(), value.clone());
        }
        self.write(&Message::Config {
            remote_dir: config.paths.remote_dir.clone(),
            commands,
        })
    }

    pub fn write(&mut self, message: &Message) -> Result<()> {
        protocol::write_message(&mut self.writer, message)
    }

    pub fn read(&mut self) -> Result<Message> {
        protocol::read_message(&mut self.reader)
    }
}

impl Drop for RemoteClient {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}
```

- [ ] **Step 2: Implement status flow**

Create `src/sync.rs`:

```rust
use crate::{
    client::RemoteClient,
    config::Config,
    diff,
    exclude::ExcludeMatcher,
    manifest::{self, Manifest},
    protocol::Message,
};
use anyhow::{bail, Result};

pub fn status(config: &Config) -> Result<()> {
    let local_manifest = local_manifest(config)?;
    let remote_manifest = remote_manifest(config)?;
    let plan = diff::calculate_diff(&local_manifest, &remote_manifest, true);

    println!("upload: {}", plan.upload.len());
    for path in &plan.upload {
        println!("  + {path}");
    }
    println!("delete: {}", plan.delete.len());
    for path in &plan.delete {
        println!("  - {path}");
    }
    println!("skipped: {}", plan.skipped);

    Ok(())
}

pub fn local_manifest(config: &Config) -> Result<Manifest> {
    let matcher = ExcludeMatcher::new(config.sync.exclude.clone())?;
    manifest::build_manifest(&config.paths.local_dir, &matcher)
}

pub fn remote_manifest(config: &Config) -> Result<Manifest> {
    let mut client = RemoteClient::connect(config)?;
    client.write(&Message::ManifestRequest)?;
    match client.read()? {
        Message::Manifest { files } => Ok(Manifest { files }),
        Message::Error { message } => bail!(message),
        other => bail!("unexpected response to manifest_request: {other:?}"),
    }
}
```

- [ ] **Step 3: Wire status in main**

Modify `src/main.rs` to add modules:

```rust
mod client;
mod sync;
```

Replace the `Command::Status` branch:

```rust
Command::Status => {
    let cfg = cfg.as_ref().expect("config loaded for local commands");
    sync::status(cfg)?;
}
```

- [ ] **Step 4: Compile**

Run: `cargo test`

Expected: PASS for unit tests. No SSH E2E test is required yet.

- [ ] **Step 5: Commit**

```bash
git add src/client.rs src/sync.rs src/main.rs
git commit -m "feat: add remote status flow"
```

## Task 7: File Upload, Delete, and Sync Flow

**Files:**
- Modify: `src/protocol.rs`
- Modify: `src/agent.rs`
- Modify: `src/sync.rs`
- Modify: `src/main.rs`
- Modify: `tests/agent_stdio_tests.rs`

- [ ] **Step 1: Add agent tests for file write and unsafe path rejection**

Append to `tests/agent_stdio_tests.rs`:

```rust
#[test]
fn agent_writes_file_payload_under_remote_dir() {
    let dir = tempfile::tempdir().unwrap();
    let mut input = Vec::new();
    protocol::write_message(
        &mut input,
        &protocol::Message::Config {
            remote_dir: dir.path().to_string_lossy().to_string(),
            commands: BTreeMap::new(),
        },
    )
    .unwrap();
    protocol::write_message(
        &mut input,
        &protocol::Message::File {
            path: "src/main.txt".into(),
            size: 5,
            hash: blake3::hash(b"hello").to_hex().to_string(),
        },
    )
    .unwrap();
    input.extend_from_slice(b"hello");

    let mut output = Vec::new();
    agent::run_agent(Cursor::new(input), &mut output).unwrap();

    assert_eq!(std::fs::read_to_string(dir.path().join("src").join("main.txt")).unwrap(), "hello");
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test --test agent_stdio_tests agent_writes_file_payload_under_remote_dir`

Expected: FAIL because `Message::File` payload is not handled.

- [ ] **Step 3: Implement file payload read/write in agent**

In `src/agent.rs`, add imports:

```rust
use crate::path_safety;
use std::io::Read as _;
```

Handle `Message::File`:

```rust
Message::File { path, size, hash } => {
    let Some(config) = &config else {
        protocol::write_message(
            &mut writer,
            &Message::Error { message: "agent config has not been received".into() },
        )?;
        continue;
    };
    path_safety::validate_relative_path(&path)?;
    let mut bytes = vec![0u8; size as usize];
    reader.read_exact(&mut bytes)?;
    let actual_hash = blake3::hash(&bytes).to_hex().to_string();
    if actual_hash != hash {
        protocol::write_message(
            &mut writer,
            &Message::Error { message: format!("hash mismatch for {path}") },
        )?;
        continue;
    }
    let target = config.remote_dir.join(path.replace('/', std::path::MAIN_SEPARATOR_STR));
    if let Some(parent) = target.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(target, bytes)?;
}
```

- [ ] **Step 4: Implement sync file sending**

In `src/sync.rs`, add:

```rust
pub fn sync(config: &Config, delete: bool) -> Result<()> {
    let local_manifest = local_manifest(config)?;
    let mut client = RemoteClient::connect(config)?;
    client.write(&Message::ManifestRequest)?;
    let remote_manifest = match client.read()? {
        Message::Manifest { files } => Manifest { files },
        Message::Error { message } => bail!(message),
        other => bail!("unexpected response to manifest_request: {other:?}"),
    };
    let plan = diff::calculate_diff(&local_manifest, &remote_manifest, delete);

    for path in &plan.upload {
        let entry = local_manifest.files.iter().find(|entry| &entry.path == path)
            .ok_or_else(|| anyhow::anyhow!("missing local manifest entry for {path}"))?;
        let bytes = std::fs::read(config.paths.local_dir.join(path.replace('/', std::path::MAIN_SEPARATOR_STR)))?;
        client.write(&Message::File {
            path: entry.path.clone(),
            size: entry.size,
            hash: entry.hash.clone(),
        })?;
        use std::io::Write;
        client.raw_write_all(&bytes)?;
    }

    client.write(&Message::SyncPlan {
        upload: Vec::new(),
        delete: if delete { plan.delete.clone() } else { Vec::new() },
    })?;

    println!("uploaded: {}", plan.upload.len());
    println!("deleted: {}", if delete { plan.delete.len() } else { 0 });
    println!("skipped: {}", plan.skipped);

    Ok(())
}
```

Add a raw write helper to `src/client.rs`:

```rust
pub fn raw_write_all(&mut self, bytes: &[u8]) -> Result<()> {
    use std::io::Write;
    self.writer.write_all(bytes)?;
    self.writer.flush()?;
    Ok(())
}
```

- [ ] **Step 5: Implement delete handling in agent**

In `src/agent.rs`, handle `Message::SyncPlan`:

```rust
Message::SyncPlan { upload: _, delete } => {
    let Some(config) = &config else {
        protocol::write_message(
            &mut writer,
            &Message::Error { message: "agent config has not been received".into() },
        )?;
        continue;
    };
    let mut deleted = 0;
    for path in delete {
        path_safety::validate_relative_path(&path)?;
        let target = config.remote_dir.join(path.replace('/', std::path::MAIN_SEPARATOR_STR));
        if target.is_file() {
            std::fs::remove_file(target)?;
            deleted += 1;
        }
    }
    protocol::write_message(&mut writer, &Message::SyncComplete {
        uploaded: 0,
        deleted,
        skipped: 0,
    })?;
}
```

- [ ] **Step 6: Wire sync in main**

Replace `Command::Sync(args)` branch:

```rust
Command::Sync(args) => {
    let cfg = cfg.as_ref().expect("config loaded for local commands");
    sync::sync(cfg, args.delete)?;
}
```

- [ ] **Step 7: Run tests**

Run: `cargo test`

Expected: PASS.

- [ ] **Step 8: Commit**

```bash
git add src/protocol.rs src/agent.rs src/sync.rs src/client.rs src/main.rs tests/agent_stdio_tests.rs
git commit -m "feat: sync files through agent protocol"
```

## Task 8: Remote Named Command Execution

**Files:**
- Modify: `src/agent.rs`
- Modify: `src/sync.rs`
- Modify: `src/main.rs`
- Modify: `tests/agent_stdio_tests.rs`

- [ ] **Step 1: Add tests for command lookup failure**

Append to `tests/agent_stdio_tests.rs`:

```rust
#[test]
fn agent_rejects_unknown_exec_name() {
    let dir = tempfile::tempdir().unwrap();
    let mut input = Vec::new();
    protocol::write_message(
        &mut input,
        &protocol::Message::Config {
            remote_dir: dir.path().to_string_lossy().to_string(),
            commands: BTreeMap::new(),
        },
    )
    .unwrap();
    protocol::write_message(&mut input, &protocol::Message::Exec { name: "build".into() }).unwrap();

    let mut output = Vec::new();
    agent::run_agent(Cursor::new(input), &mut output).unwrap();

    let response = protocol::read_message(&mut Cursor::new(output)).unwrap();
    assert_eq!(
        response,
        protocol::Message::Error { message: "commands.build is not defined".into() }
    );
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test --test agent_stdio_tests agent_rejects_unknown_exec_name`

Expected: FAIL because exec is not implemented.

- [ ] **Step 3: Implement exec handling in agent**

In `src/agent.rs`, handle `Message::Exec`:

```rust
Message::Exec { name } => {
    let Some(config) = &config else {
        protocol::write_message(
            &mut writer,
            &Message::Error { message: "agent config has not been received".into() },
        )?;
        continue;
    };
    let Some(command) = config.commands.get(&name) else {
        protocol::write_message(
            &mut writer,
            &Message::Error { message: format!("commands.{name} is not defined") },
        )?;
        continue;
    };

    let output = std::process::Command::new("powershell")
        .arg("-NoProfile")
        .arg("-ExecutionPolicy")
        .arg("Bypass")
        .arg("-Command")
        .arg(command)
        .current_dir(&config.remote_dir)
        .output()?;

    if !output.stdout.is_empty() {
        protocol::write_message(&mut writer, &Message::Output {
            stream: "stdout".into(),
            data: String::from_utf8_lossy(&output.stdout).to_string(),
        })?;
    }
    if !output.stderr.is_empty() {
        protocol::write_message(&mut writer, &Message::Output {
            stream: "stderr".into(),
            data: String::from_utf8_lossy(&output.stderr).to_string(),
        })?;
    }
    protocol::write_message(&mut writer, &Message::Exit {
        code: output.status.code().unwrap_or(1),
    })?;
}
```

- [ ] **Step 4: Implement local exec request**

In `src/sync.rs`, add:

```rust
pub fn exec(config: &Config, name: &str) -> Result<i32> {
    config.command(name)?;
    let mut client = RemoteClient::connect(config)?;
    client.write(&Message::Exec { name: name.to_string() })?;

    loop {
        match client.read()? {
            Message::Output { stream, data } => {
                if stream == "stderr" {
                    eprint!("{data}");
                } else {
                    print!("{data}");
                }
            }
            Message::Exit { code } => return Ok(code),
            Message::Error { message } => bail!(message),
            other => bail!("unexpected response to exec: {other:?}"),
        }
    }
}

pub fn sync_then_exec(config: &Config, name: &str) -> Result<i32> {
    sync(config, false)?;
    exec(config, name)
}
```

- [ ] **Step 5: Wire build/run/test in main**

In `src/main.rs`, replace branches:

```rust
Command::Build => {
    let cfg = cfg.as_ref().expect("config loaded for local commands");
    let code = sync::sync_then_exec(cfg, "build")?;
    std::process::exit(code);
}
Command::Run => {
    let cfg = cfg.as_ref().expect("config loaded for local commands");
    let code = sync::exec(cfg, "run")?;
    std::process::exit(code);
}
Command::Test => {
    let cfg = cfg.as_ref().expect("config loaded for local commands");
    let code = sync::sync_then_exec(cfg, "test")?;
    std::process::exit(code);
}
```

- [ ] **Step 6: Run tests**

Run: `cargo test`

Expected: PASS.

- [ ] **Step 7: Commit**

```bash
git add src/agent.rs src/sync.rs src/main.rs tests/agent_stdio_tests.rs
git commit -m "feat: run named remote commands"
```

## Task 9: Error Messages, Manual E2E Notes, and Final Verification

**Files:**
- Create: `docs/manual-test.md`
- Modify: `README.md` if it exists; otherwise create `README.md`

- [ ] **Step 1: Add manual test guide**

Create `docs/manual-test.md`:

```markdown
# devsync Manual Test

## Prerequisites

- Windows local machine.
- Windows remote machine.
- `ssh user@host` works from the local machine.
- `devsync.exe` is copied to the remote path configured as `connection.agent_path`.

## Test Project

Create a local test project with:

```text
devsync.toml
src/hello.txt
build.ps1
run.ps1
test.ps1
```

Use `devsync.toml.example` as the starting config.

## Checks

1. Run `devsync status`.
2. Confirm `src/hello.txt` is listed as upload.
3. Run `devsync sync`.
4. Confirm the remote `remote_dir` contains `src/hello.txt`.
5. Confirm the remote `remote_dir` does not contain `devsync.toml`, `.git`, or `.devsync`.
6. Edit `src/hello.txt`.
7. Run `devsync status`.
8. Confirm only `src/hello.txt` is listed as upload.
9. Run `devsync build`.
10. Confirm build output streams locally.
11. Run `devsync run`.
12. Confirm run output streams locally.
13. Run `devsync test`.
14. Confirm test output streams locally.
```

- [ ] **Step 2: Add README**

Create `README.md`:

```markdown
# devsync

`devsync` keeps a local Windows project as the source of truth and syncs it to a remote Windows execution copy through an SSH-launched stdio agent.

Initial commands:

```text
devsync status
devsync sync
devsync sync --delete
devsync build
devsync run
devsync test
```

See `devsync.toml.example` for configuration and `docs/manual-test.md` for the Windows OpenSSH E2E checklist.
```

- [ ] **Step 3: Run full verification**

Run: `cargo test`

Expected: PASS.

Run: `cargo run -- --help`

Expected: Help includes `status`, `sync`, `build`, `run`, `test`, and `agent`.

Run: `cargo run -- agent`

Expected: FAIL with message containing `agent requires --stdio`.

- [ ] **Step 4: Commit**

```bash
git add README.md docs/manual-test.md
git commit -m "docs: add devsync usage and manual test guide"
```

## Self-Review Checklist

- Spec coverage:
  - CLI and config are covered by Tasks 1-2.
  - Forced excludes, hash manifests, and diff are covered by Task 3.
  - Length-prefixed JSON protocol is covered by Task 4.
  - `agent --stdio` and remote manifest are covered by Task 5.
  - `status` is covered by Task 6.
  - `sync` and `sync --delete` are covered by Task 7.
  - `build`, `run`, and `test` are covered by Task 8.
  - Manual Windows OpenSSH validation is covered by Task 9.
- Out-of-scope items are intentionally absent: SFTP, daemon mode, TCP server, logs, clean, shell, Git history, bidirectional sync.
- The plan uses TDD-style task structure and keeps modules focused.
