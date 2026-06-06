use std::collections::BTreeMap;
use std::io::Cursor;

#[path = "../src/path_safety.rs"]
mod path_safety;
#[path = "../src/exclude.rs"]
mod exclude;
#[path = "../src/manifest.rs"]
mod manifest;
#[path = "../src/diff.rs"]
mod diff;
#[path = "../src/protocol.rs"]
mod protocol;
#[path = "../src/agent.rs"]
mod agent;

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
