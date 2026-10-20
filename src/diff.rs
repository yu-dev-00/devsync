use crate::manifest::Manifest;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet, HashMap};
use crate::path_safety::PathKey;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SyncPlan {
    /// Case-only renames `(from, to)`, parents before children, applied first.
    pub rename: Vec<(String, String)>,
    pub upload: Vec<String>,
    pub delete: Vec<String>,
    pub skipped: usize,
}

/// Paths share an identity when they match case-insensitively, and the local
/// spelling wins. A remote entry spelled differently is renamed rather than
/// re-uploaded or deleted: on Windows it is the same file, so deleting it would
/// delete the upload, and on Linux the stale spelling would otherwise linger.
/// Upload, delete, and skip decisions are made against the renamed remote.
pub fn calculate_diff(local: &Manifest, remote: &Manifest, include_delete: bool) -> SyncPlan {
    let remote_exact: BTreeSet<&str> = remote.files.iter().map(|entry| entry.path.as_str()).collect();
    let mut remote_by_key: BTreeMap<_, Vec<&str>> = BTreeMap::new();
    for entry in &remote.files {
        remote_by_key.entry(PathKey::new(&entry.path)).or_default().push(&entry.path);
    }

    // Only when the exact spelling is absent: beside an exact match a variant
    // can only exist on a case-sensitive remote, where it is a separate, stale
    // entry. Then the file is uploaded under the local spelling and `--delete`
    // removes the variant, instead of a rename that would collide.
    let remote_dirs: BTreeSet<&str> = remote
        .files
        .iter()
        .flat_map(|entry| entry.path.match_indices('/').map(|(end, _)| &entry.path[..end]))
        .collect();
    let mut renames = BTreeSet::new();
    for local_entry in &local.files {
        if remote_exact.contains(local_entry.path.as_str()) {
            continue;
        }
        let Some(remote_path) = remote_by_key.get(&PathKey::new(&local_entry.path)).map(|paths| paths[0]) else {
            continue;
        };
        let local_parts: Vec<&str> = local_entry.path.split('/').collect();
        for (depth, (local_part, remote_part)) in local_parts.iter().zip(remote_path.split('/')).enumerate() {
            if *local_part != remote_part {
                let parent = &local_parts[..depth];
                let from = parent.iter().copied().chain([remote_part]).collect::<Vec<_>>().join("/");
                let to = local_parts[..=depth].join("/");
                if remote_dirs.contains(to.as_str()) {
                    break;
                }
                renames.insert((depth, from, to));
            }
        }
    }
    let rename_to: HashMap<&str, &str> = renames.iter().map(|(_, from, to)| (from.as_str(), to.as_str())).collect();

    // Where each remote file lives once the renames have run.
    let renamed: BTreeMap<String, &crate::manifest::ManifestEntry> = remote
        .files
        .iter()
        .map(|entry| {
            let mut path = String::new();
            for part in entry.path.split('/') {
                if !path.is_empty() {
                    path.push('/');
                }
                path.push_str(part);
                if let Some(to) = rename_to.get(path.as_str()) {
                    path = to.to_string();
                }
            }
            (path, entry)
        })
        .collect();

    let mut upload = Vec::new();
    let mut skipped = 0;
    for local_entry in &local.files {
        match renamed.get(&local_entry.path) {
            Some(remote_entry) if remote_entry.size == local_entry.size && remote_entry.hash == local_entry.hash => {
                skipped += 1;
            }
            _ => upload.push(local_entry.path.clone()),
        }
    }

    let delete = if include_delete {
        let local_exact: BTreeSet<&str> = local.files.iter().map(|entry| entry.path.as_str()).collect();
        renamed.keys().filter(|path| !local_exact.contains(path.as_str())).cloned().collect()
    } else {
        Vec::new()
    };

    let rename = renames.into_iter().map(|(_, from, to)| (from, to)).collect();
    SyncPlan { rename, upload, delete, skipped }
}
