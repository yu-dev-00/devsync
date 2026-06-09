use std::io::Cursor;

use devsync::protocol;

#[test]
fn round_trips_json_frame() {
    let message = protocol::Message::Hello { version: 1 };
    let mut bytes = Vec::new();

    protocol::write_message(&mut bytes, &message).unwrap();
    let decoded = protocol::read_message(&mut Cursor::new(bytes)).unwrap();

    assert_eq!(decoded, message);
}

#[test]
fn file_message_declares_payload_size() {
    let message = protocol::Message::File {
        path: "src/main.txt".into(),
        size: 5,
        hash: "abc".into(),
    };

    let json = serde_json::to_string(&message).unwrap();

    assert!(json.contains("\"type\":\"file\""));
    assert!(json.contains("\"size\":5"));
}
