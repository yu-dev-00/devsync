//! Remembers the BLAKE3 hash of every file a manifest walk has already read, so
//! an unchanged tree is not re-read and re-hashed on every `status`/`sync`.
//!
//! **This does not make the diff mtime-based.** Uploads are still decided by
//! comparing content hashes between the two machines. mtime is used only to ask
//! "is this the same file *this* machine hashed last time?" — a local, same-
//! filesystem comparison, never a comparison of a local timestamp against a
//! remote one. The Windows timestamp precision and timezone problems that made
//! mtime unusable for diffing do not apply to that question.
//!
//! The cache lives at `<root>/.devsync/state`, inside a forced exclude, so it is
//! never uploaded and `sync --delete` never removes it. Every operation is
//! best-effort: a missing, unreadable, or corrupt cache simply means everything
//! is hashed, which is exactly the behavior from before this existed.

use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
    time::UNIX_EPOCH,
};

/// Bumped when the entry format changes; an older file is discarded rather than
/// misread. Unrelated to `PROTOCOL_VERSION` — the cache never goes on the wire.
const CACHE_VERSION: u32 = 1;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct CacheEntry {
    size: u64,
    /// Nanoseconds since the Unix epoch. Absent when the platform or file has no
    /// usable timestamp, which makes the entry permanently unusable for reuse.
    mtime_ns: Option<u64>,
    hash: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct HashCache {
    version: u32,
    entries: BTreeMap<String, CacheEntry>,
}

/// Read a file's modification time as nanoseconds since the Unix epoch.
/// Returns `None` for timestamps that predate the epoch or cannot be read; such
/// files are simply always hashed.
pub fn modified_ns(metadata: &fs::Metadata) -> Option<u64> {
    metadata
        .modified()
        .ok()
        .and_then(|time| time.duration_since(UNIX_EPOCH).ok())
        .and_then(|since_epoch| u64::try_from(since_epoch.as_nanos()).ok())
}

fn cache_path(root: &Path) -> PathBuf {
    root.join(".devsync").join("state")
}

impl HashCache {
    pub fn new() -> Self {
        Self { version: CACHE_VERSION, entries: BTreeMap::new() }
    }

    /// Load the cache for `root`. Any problem — no file, unreadable, malformed,
    /// written by a different cache version — yields an empty cache.
    pub fn load(root: &Path) -> Self {
        let Ok(raw) = fs::read_to_string(cache_path(root)) else {
            return Self::new();
        };
        match serde_json::from_str::<HashCache>(&raw) {
            Ok(cache) if cache.version == CACHE_VERSION => cache,
            _ => Self::new(),
        }
    }

    /// The recorded hash for `path`, but only if size *and* mtime still match —
    /// that is, only if this is the same file content this machine hashed
    /// before. A file whose timestamp could not be read is never reusable.
    pub fn reusable_hash(&self, path: &str, size: u64, mtime_ns: Option<u64>) -> Option<&str> {
        let mtime_ns = mtime_ns?;
        let entry = self.entries.get(path)?;
        (entry.size == size && entry.mtime_ns == Some(mtime_ns)).then_some(entry.hash.as_str())
    }

    pub fn record(&mut self, path: String, size: u64, mtime_ns: Option<u64>, hash: String) {
        self.entries.insert(path, CacheEntry { size, mtime_ns, hash });
    }

    /// Write the cache next to the tree it describes. Failures are ignored: the
    /// tree may be read-only, and losing a cache costs speed, never correctness.
    pub fn save(&self, root: &Path) {
        let path = cache_path(root);
        let Some(parent) = path.parent() else { return };
        if fs::create_dir_all(parent).is_err() {
            return;
        }
        if let Ok(serialized) = serde_json::to_vec(self) {
            let _ = fs::write(path, serialized);
        }
    }
}
