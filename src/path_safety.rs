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
