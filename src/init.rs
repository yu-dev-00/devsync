//! `devsync init` — scaffold a project's `devsync.toml`, and optionally install
//! the Claude Code skill.
//!
//! The config template is embedded rather than read from disk because
//! `devsync.toml.example` ships in the source tree, while what gets installed is
//! a lone `devsync.exe`. Without embedding, starting a new project means going
//! to find wherever the devsync sources were checked out.
//!
//! The skill is installed to the user's home directory, not into the project.
//! It describes how devsync works, which is the same everywhere and changes when
//! devsync changes; a per-project copy would go stale and keep advising the old
//! behavior — including how to read errors that no longer occur.

use anyhow::{bail, Context, Result};
use std::path::{Path, PathBuf};

/// Single source of truth with the documented example, so the two cannot drift.
const CONFIG_TEMPLATE: &str = include_str!("../devsync.toml.example");
const SKILL: &str = include_str!("../skills/devsync/SKILL.md");

/// The cache lives here and is machine-local; committing it would put one
/// machine's timestamps in everyone's history.
const GITIGNORE_ENTRY: &str = ".devsync/";

#[derive(Debug, Clone, Default)]
pub struct InitOptions {
    pub host: Option<String>,
    pub user: Option<String>,
    pub remote_dir: Option<String>,
    pub force: bool,
    pub install_skill: bool,
}

pub fn run(config_path: &Path, options: &InitOptions) -> Result<()> {
    if config_path.exists() && !options.force {
        bail!(
            "{} already exists; pass --force to overwrite it",
            config_path.display()
        );
    }

    let rendered = render_config(options)?;
    if let Some(parent) = config_path.parent() {
        if !parent.as_os_str().is_empty() {
            std::fs::create_dir_all(parent)
                .with_context(|| format!("failed to create {}", parent.display()))?;
        }
    }
    std::fs::write(config_path, rendered)
        .with_context(|| format!("failed to write {}", config_path.display()))?;
    println!("wrote {}", config_path.display());

    let project_dir = config_path.parent().unwrap_or(Path::new("."));
    match update_gitignore(project_dir)? {
        GitignoreOutcome::Added(path) => println!("added {GITIGNORE_ENTRY} to {}", path.display()),
        GitignoreOutcome::AlreadyPresent => {}
        GitignoreOutcome::NotAGitRepository => {}
    }

    if options.install_skill {
        let installed = install_skill(&home_dir()?)?;
        println!("installed the Claude Code skill to {}", installed.display());
        println!("  (re-run with --install-skill after upgrading devsync to refresh it)");
    }

    if options.host.is_none() || options.user.is_none() || options.remote_dir.is_none() {
        println!(
            "\nEdit {} and fill in the connection details, then run `devsync status`.",
            config_path.display()
        );
    } else {
        println!("\nRun `devsync status` to see what a first sync would do.");
    }

    Ok(())
}

fn render_config(options: &InitOptions) -> Result<String> {
    let mut rendered = CONFIG_TEMPLATE.to_string();
    if let Some(host) = &options.host {
        rendered = substitute(&rendered, "host = \"remote-pc\"", "host", host)?;
    }
    if let Some(user) = &options.user {
        rendered = substitute(&rendered, "user = \"user\"", "user", user)?;
    }
    if let Some(remote_dir) = &options.remote_dir {
        rendered = substitute(
            &rendered,
            "remote_dir = \"C:\\\\work\\\\project\"",
            "remote_dir",
            remote_dir,
        )?;
    }
    Ok(rendered)
}

/// Replace a placeholder line. Failing loudly when the placeholder is missing
/// turns a drifted template into an error at build/test time instead of a config
/// that silently keeps the example's values.
fn substitute(text: &str, placeholder: &str, key: &str, value: &str) -> Result<String> {
    if !text.contains(placeholder) {
        bail!(
            "devsync.toml.example no longer contains `{placeholder}`; \
             the init template needs updating to match it"
        );
    }
    Ok(text.replace(placeholder, &format!("{key} = \"{}\"", escape_toml(value))))
}

/// Escape for a TOML basic string. Windows paths are the reason this matters:
/// an unescaped `C:\work` would make `\w` an invalid escape and fail to parse.
fn escape_toml(value: &str) -> String {
    value.replace('\\', "\\\\").replace('"', "\\\"")
}

enum GitignoreOutcome {
    Added(PathBuf),
    AlreadyPresent,
    NotAGitRepository,
}

/// Only touches `.gitignore` inside a git repository — creating one elsewhere
/// would be litter in a directory that has nothing to do with git.
fn update_gitignore(project_dir: &Path) -> Result<GitignoreOutcome> {
    if !project_dir.join(".git").exists() {
        return Ok(GitignoreOutcome::NotAGitRepository);
    }

    let path = project_dir.join(".gitignore");
    let existing = std::fs::read_to_string(&path).unwrap_or_default();
    if existing
        .lines()
        .any(|line| line.trim().trim_end_matches('/') == GITIGNORE_ENTRY.trim_end_matches('/'))
    {
        return Ok(GitignoreOutcome::AlreadyPresent);
    }

    let mut updated = existing;
    if !updated.is_empty() && !updated.ends_with('\n') {
        updated.push('\n');
    }
    updated.push_str(GITIGNORE_ENTRY);
    updated.push('\n');
    std::fs::write(&path, updated)
        .with_context(|| format!("failed to update {}", path.display()))?;
    Ok(GitignoreOutcome::Added(path))
}

/// Takes the home directory as a parameter so tests never write into the real one.
pub fn install_skill(home: &Path) -> Result<PathBuf> {
    let directory = home.join(".claude").join("skills").join("devsync");
    std::fs::create_dir_all(&directory)
        .with_context(|| format!("failed to create {}", directory.display()))?;
    let path = directory.join("SKILL.md");
    // Overwritten on purpose: the skill exists to stay in step with the binary
    // that ships it, so refreshing it is the point of re-running this.
    std::fs::write(&path, SKILL).with_context(|| format!("failed to write {}", path.display()))?;
    Ok(path)
}

fn home_dir() -> Result<PathBuf> {
    std::env::var_os("USERPROFILE")
        .or_else(|| std::env::var_os("HOME"))
        .map(PathBuf::from)
        .context("could not determine the home directory (neither USERPROFILE nor HOME is set)")
}
