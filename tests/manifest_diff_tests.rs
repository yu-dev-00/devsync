use std::fs;

use devsync::{diff, exclude, manifest, path_safety};

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

#[test]
fn build_manifest_errors_on_unreadable_root() {
    // A non-existent root makes WalkDir yield an error on first iteration;
    // build_manifest must surface it as Err, not silently return an empty manifest.
    let dir = tempfile::tempdir().unwrap();
    let missing = dir.path().join("does-not-exist");

    let matcher = exclude::ExcludeMatcher::new(vec![]).unwrap();
    let result = manifest::build_manifest(&missing, &matcher);

    assert!(result.is_err(), "expected build_manifest to error on an unreadable/missing root");
}
