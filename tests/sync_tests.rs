use std::fs;

#[test]
fn build_file_message_derives_size_and_hash_from_current_bytes() {
    let dir = tempfile::tempdir().unwrap();
    fs::create_dir_all(dir.path().join("src")).unwrap();
    fs::write(dir.path().join("src").join("main.txt"), "hello world").unwrap();

    let (message, bytes) = devsync::sync::build_file_message(dir.path(), "src/main.txt").unwrap();

    assert_eq!(bytes, b"hello world");
    match message {
        devsync::protocol::Message::File { path, size, hash } => {
            assert_eq!(path, "src/main.txt");
            // size and hash MUST match the bytes actually read, not any stale manifest value.
            assert_eq!(size, bytes.len() as u64);
            assert_eq!(size, 11);
            assert_eq!(hash, blake3::hash(&bytes).to_hex().to_string());
        }
        other => panic!("expected File message, got {other:?}"),
    }
}
