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
