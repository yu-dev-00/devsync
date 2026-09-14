use devsync::{client, diff, exclude::ExcludeMatcher, manifest, protocol::{self, Message}, sync};
use std::{collections::BTreeMap, fs, io::{BufReader, BufWriter}, path::Path, process::{Child, Command, Stdio}};

// Reap the real agent even when a regression causes an assertion or I/O failure.
struct Agent(Child);
impl Drop for Agent {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

fn synchronize(local: &Path, remote: &Path, delete: bool, exclude: Vec<String>) -> anyhow::Result<(usize, usize)> {
    let matcher = ExcludeMatcher::new(exclude.clone())?;
    let local_manifest = manifest::build_manifest(local, &matcher)?;
    let mut agent = Agent(Command::new(env!("CARGO_BIN_EXE_devsync"))
        .args(["agent", "--stdio"])
        .stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::null()).spawn()?);
    let mut writer = BufWriter::new(agent.0.stdin.take().unwrap());
    let mut reader = BufReader::new(agent.0.stdout.take().unwrap());
    client::perform_handshake(&mut reader, &mut writer)?;
    protocol::write_message(&mut writer, &Message::Config {
        remote_dir: remote.to_string_lossy().into_owned(), commands: BTreeMap::new(), exclude,
    })?;
    protocol::write_message(&mut writer, &Message::ManifestRequest)?;
    let remote_manifest = match protocol::read_message(&mut reader)? {
        Message::Manifest { files } => manifest::Manifest { files },
        other => anyhow::bail!("unexpected manifest: {other:?}"),
    };
    let plan = diff::calculate_diff(&local_manifest, &remote_manifest, delete);
    sync::apply_plan(local, &plan, &mut reader, &mut writer)
}

#[test]
fn delete_sync_replaces_directory_with_file_and_counts_all_deletes() {
    let local = tempfile::tempdir().unwrap();
    let remote = tempfile::tempdir().unwrap();
    fs::write(local.path().join("item"), "new").unwrap();
    fs::create_dir_all(remote.path().join("ITEM/nested")).unwrap();
    fs::write(remote.path().join("ITEM/nested/old.txt"), "old").unwrap();
    fs::write(remote.path().join("unrelated.txt"), "stale").unwrap();
    let result = synchronize(local.path(), remote.path(), true, vec![]);
    assert!(result.is_ok(), "sync failed: {result:?}");
    assert_eq!(result.unwrap(), (1, 2));
    assert_eq!(fs::read_to_string(remote.path().join("item")).unwrap(), "new");
    assert!(!remote.path().join("unrelated.txt").exists());
    assert_eq!(synchronize(local.path(), remote.path(), true, vec![]).unwrap(), (0, 0));
}

#[test]
fn delete_sync_replaces_file_with_directory() {
    let local = tempfile::tempdir().unwrap();
    let remote = tempfile::tempdir().unwrap();
    fs::create_dir_all(local.path().join("item")).unwrap();
    fs::write(local.path().join("item/new.txt"), "new").unwrap();
    fs::write(remote.path().join("ITEM"), "old").unwrap();
    assert_eq!(synchronize(local.path(), remote.path(), true, vec![]).unwrap(), (1, 1));
    assert_eq!(fs::read_to_string(remote.path().join("item/new.txt")).unwrap(), "new");
}

#[test]
fn type_changes_without_delete_preserve_remote_files() {
    for directory_to_file in [true, false] {
        let local = tempfile::tempdir().unwrap();
        let remote = tempfile::tempdir().unwrap();
        let old = if directory_to_file {
            fs::write(local.path().join("item"), "new").unwrap();
            fs::create_dir_all(remote.path().join("item")).unwrap();
            remote.path().join("item/old.txt")
        } else {
            fs::create_dir_all(local.path().join("item")).unwrap();
            fs::write(local.path().join("item/new.txt"), "new").unwrap();
            remote.path().join("item")
        };
        fs::write(&old, "keep").unwrap();
        assert!(synchronize(local.path(), remote.path(), false, vec![]).is_err());
        assert_eq!(fs::read_to_string(old).unwrap(), "keep");
    }
}

#[test]
fn directory_replacement_preserves_excluded_files_and_empty_directories() {
    for has_file in [true, false] {
        let local = tempfile::tempdir().unwrap();
        let remote = tempfile::tempdir().unwrap();
        fs::write(local.path().join("item"), "new").unwrap();
        fs::create_dir_all(remote.path().join("item/BIN")).unwrap();
        if has_file {
            fs::write(remote.path().join("item/BIN/keep.dll"), "artifact").unwrap();
        }
        assert!(synchronize(local.path(), remote.path(), true, vec!["bin".into()]).is_err());
        assert!(remote.path().join("item/BIN").is_dir());
        if has_file {
            assert_eq!(fs::read_to_string(remote.path().join("item/BIN/keep.dll")).unwrap(), "artifact");
        }
    }
}

#[test]
fn case_only_rename_keeps_remote_content_after_delete_sync() {
    let local = tempfile::tempdir().unwrap();
    let remote = tempfile::tempdir().unwrap();
    fs::write(local.path().join("foo.txt"), "new").unwrap();
    fs::write(remote.path().join("Foo.txt"), "old").unwrap();
    assert_eq!(synchronize(local.path(), remote.path(), true, vec![]).unwrap(), (1, 0));
    assert_eq!(fs::read_to_string(remote.path().join("foo.txt")).unwrap(), "new");
    assert_eq!(synchronize(local.path(), remote.path(), true, vec![]).unwrap(), (0, 0));
}
