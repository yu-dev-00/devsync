use devsync::{exclude::ExcludeMatcher, hash_cache::HashCache, manifest};
use std::fs;

fn matcher() -> ExcludeMatcher {
    ExcludeMatcher::new(vec![]).unwrap()
}

/// The cache exists to skip re-reading unchanged files, but it must never change
/// what the manifest says. Same tree, second walk, identical result.
#[test]
fn cached_walk_produces_the_same_manifest() {
    let dir = tempfile::tempdir().unwrap();
    fs::create_dir_all(dir.path().join("src")).unwrap();
    fs::write(dir.path().join("src").join("a.txt"), "alpha").unwrap();
    fs::write(dir.path().join("b.bin"), [0u8, 159, 146, 150]).unwrap();

    let cold = manifest::build_manifest(dir.path(), &matcher()).unwrap();
    let warm = manifest::build_manifest(dir.path(), &matcher()).unwrap();

    assert_eq!(cold, warm, "a cache hit must be indistinguishable from a rehash");
    assert!(dir.path().join(".devsync").join("state").is_file(), "cache must be written");
}

/// A cache that survived an edit would make devsync upload stale content — or
/// skip uploading changed content, which is worse because it is silent.
#[test]
fn edited_file_is_rehashed_despite_the_cache() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("a.txt");
    fs::write(&file, "before").unwrap();

    let before = manifest::build_manifest(dir.path(), &matcher()).unwrap();

    // Force a distinct mtime: NTFS is fine-grained, but a same-tick rewrite
    // would make this test prove nothing.
    std::thread::sleep(std::time::Duration::from_millis(20));
    fs::write(&file, "after!").unwrap(); // same length, different content

    let after = manifest::build_manifest(dir.path(), &matcher()).unwrap();

    assert_eq!(before.files[0].size, after.files[0].size, "sizes match, so mtime must catch it");
    assert_ne!(
        before.files[0].hash, after.files[0].hash,
        "the edit must be visible in the hash, not masked by the cache"
    );
    assert_eq!(after.files[0].hash, blake3::hash(b"after!").to_hex().to_string());
}

/// The cache file lives under `.devsync`, a forced exclude, so writing it must
/// not make it appear in the manifest — otherwise it would sync itself.
#[test]
fn cache_file_never_enters_the_manifest() {
    let dir = tempfile::tempdir().unwrap();
    fs::write(dir.path().join("a.txt"), "alpha").unwrap();

    manifest::build_manifest(dir.path(), &matcher()).unwrap();
    let second = manifest::build_manifest(dir.path(), &matcher()).unwrap();

    assert_eq!(second.files.len(), 1);
    assert_eq!(second.files[0].path, "a.txt");
}

/// Entries for deleted files must not accumulate; the cache is rebuilt from the
/// files actually walked.
#[test]
fn removed_files_drop_out_of_the_cache() {
    let dir = tempfile::tempdir().unwrap();
    fs::write(dir.path().join("keep.txt"), "keep").unwrap();
    fs::write(dir.path().join("gone.txt"), "gone").unwrap();

    manifest::build_manifest(dir.path(), &matcher()).unwrap();
    fs::remove_file(dir.path().join("gone.txt")).unwrap();
    manifest::build_manifest(dir.path(), &matcher()).unwrap();

    let cache = HashCache::load(dir.path());
    assert!(cache.reusable_hash("gone.txt", 4, Some(0)).is_none());
}

/// A corrupt or truncated cache must degrade to "hash everything", never fail
/// the walk — the tree is the source of truth, the cache is only an optimization.
#[test]
fn corrupt_cache_is_ignored_rather_than_fatal() {
    let dir = tempfile::tempdir().unwrap();
    fs::write(dir.path().join("a.txt"), "alpha").unwrap();
    fs::create_dir_all(dir.path().join(".devsync")).unwrap();
    fs::write(dir.path().join(".devsync").join("state"), "{ not json at all").unwrap();

    let built = manifest::build_manifest(dir.path(), &matcher()).unwrap();

    assert_eq!(built.files.len(), 1);
    assert_eq!(built.files[0].hash, blake3::hash(b"alpha").to_hex().to_string());
}

/// A hash may only be reused when the file still looks identical. Size and
/// mtime are both part of that judgement, and a missing mtime disqualifies it.
#[test]
fn reuse_requires_matching_size_and_mtime() {
    let mut cache = HashCache::new();
    cache.record("a.txt".into(), 10, Some(500), "cafe".into());

    assert_eq!(cache.reusable_hash("a.txt", 10, Some(500)), Some("cafe"));
    assert_eq!(cache.reusable_hash("a.txt", 11, Some(500)), None, "size changed");
    assert_eq!(cache.reusable_hash("a.txt", 10, Some(501)), None, "mtime changed");
    assert_eq!(cache.reusable_hash("a.txt", 10, None), None, "no timestamp to trust");
    assert_eq!(cache.reusable_hash("other.txt", 10, Some(500)), None, "different file");
}
