use anyhow::{bail, Result};
use std::path::{Component, Path};
use std::cmp::Ordering;

/// Windows ordinal case-insensitive identity, shared by diffing and excludes.
/// Keep the original spelling separately for display and filesystem operations.
#[derive(Debug, Clone)]
pub(crate) struct PathKey(Vec<u16>);

impl PathKey {
    pub(crate) fn new(path: &str) -> Self {
        let path = path.replace('\\', "/");
        #[cfg(not(windows))]
        let path = path.to_lowercase();
        Self(path.encode_utf16().collect())
    }
}

impl Ord for PathKey {
    fn cmp(&self, other: &Self) -> Ordering {
        #[cfg(windows)]
        {
            // Explicit lengths keep this independent of NUL termination and locale.
            let result = unsafe {
                windows_sys::Win32::Globalization::CompareStringOrdinal(
                    self.0.as_ptr(), self.0.len().try_into().expect("path too long"),
                    other.0.as_ptr(), other.0.len().try_into().expect("path too long"), 1,
                )
            };
            match result {
                1 => Ordering::Less,
                2 => Ordering::Equal,
                3 => Ordering::Greater,
                _ => panic!("CompareStringOrdinal failed"),
            }
        }
        #[cfg(not(windows))]
        self.0.cmp(&other.0)
    }
}

impl PartialOrd for PathKey {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> { Some(self.cmp(other)) }
}
impl PartialEq for PathKey {
    fn eq(&self, other: &Self) -> bool { self.cmp(other) == Ordering::Equal }
}
impl Eq for PathKey {}

pub(crate) fn is_same_or_descendant(path: &str, parent: &str) -> bool {
    let mut parts = path.split('/');
    parent.split('/').all(|part| {
        parts.next().is_some_and(|next| PathKey::new(next) == PathKey::new(part))
    })
}

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
