use crate::{exclude::ExcludeMatcher, hash_cache::{self, HashCache}, path_safety};
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
    // Reuse hashes for files this machine has already read and that have not
    // changed since. The rebuilt cache is written from scratch below, so entries
    // for deleted files disappear instead of accumulating forever.
    let previous = HashCache::load(root);
    let mut current = HashCache::new();

    let mut files = Vec::new();
    for entry in WalkDir::new(root) {
        let entry = entry?;            // propagate traversal errors instead of swallowing
        if !entry.file_type().is_file() {
            continue;
        }
        let relative_path = entry.path().strip_prefix(root)?;
        let normalized = path_safety::normalize_relative_path(relative_path)?;
        if excludes.is_excluded(&normalized) {
            continue;
        }

        let metadata = entry.metadata()?;
        let size = metadata.len();
        let mtime_ns = hash_cache::modified_ns(&metadata);

        let hash = match previous.reusable_hash(&normalized, size, mtime_ns) {
            Some(cached) => cached.to_string(),
            None => blake3::hash(&fs::read(entry.path())?).to_hex().to_string(),
        };

        current.record(normalized.clone(), size, mtime_ns, hash.clone());
        files.push(ManifestEntry { path: normalized, size, hash });
    }

    files.sort_by(|a, b| a.path.cmp(&b.path));
    current.save(root);
    Ok(Manifest { files })
}
