use std::io::Cursor;

#[test]
fn perform_handshake_succeeds_on_matching_version() {
    // Pre-fill the reader with the agent's Hello reply.
    let mut agent_reply = Vec::new();
    devsync::protocol::write_message(
        &mut agent_reply,
        &devsync::protocol::Message::Hello { version: devsync::protocol::PROTOCOL_VERSION },
    )
    .unwrap();

    let mut reader = Cursor::new(agent_reply);
    let mut writer: Vec<u8> = Vec::new();

    devsync::client::perform_handshake(&mut reader, &mut writer).unwrap();

    // The client must have written its own Hello at the current version.
    let sent = devsync::protocol::read_message(&mut Cursor::new(writer)).unwrap();
    assert_eq!(sent, devsync::protocol::Message::Hello { version: devsync::protocol::PROTOCOL_VERSION });
}

#[test]
fn perform_handshake_fails_on_version_mismatch() {
    let mut agent_reply = Vec::new();
    devsync::protocol::write_message(
        &mut agent_reply,
        &devsync::protocol::Message::Hello { version: 999 },
    )
    .unwrap();

    let mut reader = Cursor::new(agent_reply);
    let mut writer: Vec<u8> = Vec::new();

    let err = devsync::client::perform_handshake(&mut reader, &mut writer).unwrap_err().to_string();
    assert!(err.contains("version"), "error should mention version, got: {err}");
}

#[test]
fn perform_handshake_fails_on_error_reply() {
    let mut agent_reply = Vec::new();
    devsync::protocol::write_message(
        &mut agent_reply,
        &devsync::protocol::Message::Error {
            message: "unsupported protocol version: 1 (agent supports 2)".into(),
        },
    )
    .unwrap();

    let mut reader = Cursor::new(agent_reply);
    let mut writer: Vec<u8> = Vec::new();

    let err = devsync::client::perform_handshake(&mut reader, &mut writer).unwrap_err().to_string();
    assert!(err.contains("unsupported protocol version"), "error should forward the agent message, got: {err}");
}

#[test]
fn perform_handshake_fails_on_unexpected_reply() {
    // Agent replies with something that isn't a Hello/Error — handshake must fail.
    let mut agent_reply = Vec::new();
    devsync::protocol::write_message(
        &mut agent_reply,
        &devsync::protocol::Message::ManifestRequest,
    )
    .unwrap();

    let mut reader = Cursor::new(agent_reply);
    let mut writer: Vec<u8> = Vec::new();

    let result = devsync::client::perform_handshake(&mut reader, &mut writer);
    assert!(result.is_err(), "an unexpected reply must fail the handshake");
}
