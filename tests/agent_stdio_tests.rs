use std::collections::BTreeMap;
use std::io::Cursor;

use devsync::{agent, protocol};

// Step 1: failing test – agent_writes_file_payload_under_remote_dir
#[test]
fn agent_writes_file_payload_under_remote_dir() {
    let dir = tempfile::tempdir().unwrap();
    let mut input = Vec::new();
    protocol::write_message(
        &mut input,
        &protocol::Message::Config {
            remote_dir: dir.path().to_string_lossy().to_string(),
            commands: BTreeMap::new(),
        },
    )
    .unwrap();
    protocol::write_message(
        &mut input,
        &protocol::Message::File {
            path: "src/main.txt".into(),
            size: 5,
            hash: blake3::hash(b"hello").to_hex().to_string(),
        },
    )
    .unwrap();
    input.extend_from_slice(b"hello");

    let mut output = Vec::new();
    agent::run_agent(Cursor::new(input), &mut output).unwrap();

    assert_eq!(
        std::fs::read_to_string(dir.path().join("src").join("main.txt")).unwrap(),
        "hello"
    );
}

// Step 8 regression: no-config File must consume payload to keep stream aligned
#[test]
fn agent_consumes_file_payload_even_without_config() {
    // A File arriving before Config must still consume its payload so a
    // following framed message is parsed correctly.
    let mut input = Vec::new();
    protocol::write_message(
        &mut input,
        &protocol::Message::File { path: "a.txt".into(), size: 3, hash: "x".into() },
    )
    .unwrap();
    input.extend_from_slice(b"abc");
    protocol::write_message(&mut input, &protocol::Message::ManifestRequest).unwrap();

    let mut output = Vec::new();
    agent::run_agent(Cursor::new(input), &mut output).unwrap();

    // Two responses expected: Error (no config for File) then Error (no config for ManifestRequest).
    let mut cursor = Cursor::new(output);
    let first = protocol::read_message(&mut cursor).unwrap();
    let second = protocol::read_message(&mut cursor).unwrap();
    assert_eq!(
        first,
        protocol::Message::Error { message: "agent config has not been received".into() }
    );
    assert_eq!(
        second,
        protocol::Message::Error { message: "agent config has not been received".into() }
    );
}

#[test]
fn agent_rejects_manifest_before_config() {
    let mut input = Vec::new();
    protocol::write_message(&mut input, &protocol::Message::ManifestRequest).unwrap();

    let mut output = Vec::new();
    agent::run_agent(Cursor::new(input), &mut output).unwrap();

    let response = protocol::read_message(&mut Cursor::new(output)).unwrap();
    assert_eq!(
        response,
        protocol::Message::Error { message: "agent config has not been received".into() }
    );
}

#[test]
fn agent_accepts_config_and_returns_manifest() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("remote.txt"), "remote").unwrap();

    let mut commands = BTreeMap::new();
    commands.insert("run".to_string(), "echo ok".to_string());

    let mut input = Vec::new();
    protocol::write_message(
        &mut input,
        &protocol::Message::Config {
            remote_dir: dir.path().to_string_lossy().to_string(),
            commands,
        },
    )
    .unwrap();
    protocol::write_message(&mut input, &protocol::Message::ManifestRequest).unwrap();

    let mut output = Vec::new();
    agent::run_agent(Cursor::new(input), &mut output).unwrap();

    let response = protocol::read_message(&mut Cursor::new(output)).unwrap();
    match response {
        protocol::Message::Manifest { files } => {
            assert_eq!(files.len(), 1);
            assert_eq!(files[0].path, "remote.txt");
        }
        other => panic!("unexpected response: {other:?}"),
    }
}

#[test]
fn agent_rejects_unsafe_file_path_without_terminating() {
    let dir = tempfile::tempdir().unwrap();
    let mut input = Vec::new();
    protocol::write_message(
        &mut input,
        &protocol::Message::Config {
            remote_dir: dir.path().to_string_lossy().to_string(),
            commands: BTreeMap::new(),
        },
    )
    .unwrap();
    // Unsafe path with a 3-byte payload.
    protocol::write_message(
        &mut input,
        &protocol::Message::File { path: "../evil.txt".into(), size: 3, hash: "x".into() },
    )
    .unwrap();
    input.extend_from_slice(b"abc");
    // A following request must still be processed (agent not terminated, stream aligned).
    protocol::write_message(&mut input, &protocol::Message::ManifestRequest).unwrap();

    let mut output = Vec::new();
    agent::run_agent(Cursor::new(input), &mut output).unwrap();

    let mut cursor = Cursor::new(output);
    let first = protocol::read_message(&mut cursor).unwrap();
    // First response: an Error about the unsafe path (not a process termination).
    match first {
        protocol::Message::Error { message } => assert!(message.contains("unsafe") || message.contains("relative") || message.contains("absolute")),
        other => panic!("expected Error for unsafe path, got {other:?}"),
    }
    // Second response: the agent is still alive and answers the ManifestRequest
    // (a Manifest, since config was provided).
    let second = protocol::read_message(&mut cursor).unwrap();
    assert!(matches!(second, protocol::Message::Manifest { .. }));
}

#[test]
fn agent_rejects_unknown_exec_name() {
    let dir = tempfile::tempdir().unwrap();
    let mut input = Vec::new();
    protocol::write_message(
        &mut input,
        &protocol::Message::Config {
            remote_dir: dir.path().to_string_lossy().to_string(),
            commands: BTreeMap::new(),
        },
    )
    .unwrap();
    protocol::write_message(&mut input, &protocol::Message::Exec { name: "build".into() }).unwrap();

    let mut output = Vec::new();
    agent::run_agent(Cursor::new(input), &mut output).unwrap();

    let response = protocol::read_message(&mut Cursor::new(output)).unwrap();
    assert_eq!(
        response,
        protocol::Message::Error { message: "commands.build is not defined".into() }
    );
}

#[test]
fn agent_rejects_exec_before_config() {
    let mut input = Vec::new();
    protocol::write_message(&mut input, &protocol::Message::Exec { name: "run".into() }).unwrap();

    let mut output = Vec::new();
    agent::run_agent(Cursor::new(input), &mut output).unwrap();

    let response = protocol::read_message(&mut Cursor::new(output)).unwrap();
    assert_eq!(
        response,
        protocol::Message::Error { message: "agent config has not been received".into() }
    );
}
