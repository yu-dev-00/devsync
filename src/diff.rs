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
