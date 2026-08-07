//! `OutputDecoder` decides an encoding per line instead of taking one for the
//! whole stream, because nothing makes the writers on a single pipe agree:
//! PowerShell and the classic console tools emit the console code page, while
//! anything built in Rust or Go (`uv`, `cargo`) emits UTF-8 regardless of it.
//!
//! These tests pin CP932 as the console encoding rather than asking Windows, so
//! they assert the same thing on any machine.

use devsync::agent::OutputDecoder;

fn cp932(text: &str) -> Vec<u8> {
    let (bytes, _encoding, had_unmappable) = encoding_rs::SHIFT_JIS.encode(text);
    assert!(!had_unmappable, "test fixture must be representable in CP932: {text:?}");
    bytes.into_owned()
}

fn decoder() -> OutputDecoder {
    OutputDecoder::new(encoding_rs::SHIFT_JIS)
}

// ── one encoding at a time ─────────────────────────────────────────────────

/// The regression that started this: `uv` writes UTF-8 whatever the code page
/// is, and decoding it as CP932 turned 信頼されていない into mojibake.
#[test]
fn utf8_output_survives_a_cp932_console() {
    let mut decoder = decoder();
    let decoded = decoder.push("信頼されていないマウントポイント\r\n".as_bytes());
    assert_eq!(decoded, "信頼されていないマウントポイント\r\n");
}

/// The regression before that: PowerShell's own output is console-code-page
/// encoded, and reading it as UTF-8 replaced every character with U+FFFD.
#[test]
fn cp932_output_survives() {
    let mut decoder = decoder();
    let decoded = decoder.push(&cp932("日本語テスト\r\n"));
    assert_eq!(decoded, "日本語テスト\r\n");
}

#[test]
fn ascii_is_unchanged_and_decodes_the_same_either_way() {
    let mut decoder = decoder();
    assert_eq!(decoder.push(b"Compiling devsync v0.1.0\r\n"), "Compiling devsync v0.1.0\r\n");
}

// ── both encodings in one stream ───────────────────────────────────────────

/// The case that made a single fixed encoding untenable: one real run produced
/// a UTF-8 error from `uv` and CP932 experiment output from PowerShell. Here
/// they arrive in the *same* chunk, which is why the verdict is per line.
#[test]
fn utf8_and_cp932_lines_in_one_chunk_both_survive() {
    let mut chunk = Vec::new();
    chunk.extend_from_slice("error: 信頼されていないマウントポイント\r\n".as_bytes());
    chunk.extend_from_slice(&cp932("実験開始: 75項目\r\n"));
    chunk.extend_from_slice("warning: 続行します\r\n".as_bytes());

    let mut decoder = decoder();
    let decoded = decoder.push(&chunk);

    assert_eq!(
        decoded,
        "error: 信頼されていないマウントポイント\r\n実験開始: 75項目\r\nwarning: 続行します\r\n"
    );
}

#[test]
fn the_encoding_can_change_between_chunks() {
    let mut decoder = decoder();
    assert_eq!(decoder.push(&cp932("実験開始\r\n")), "実験開始\r\n");
    assert_eq!(decoder.push("信頼されていない\r\n".as_bytes()), "信頼されていない\r\n");
    assert_eq!(decoder.push(&cp932("実験終了\r\n")), "実験終了\r\n");
}

// ── chunk boundaries ───────────────────────────────────────────────────────

/// A read can end anywhere, including the middle of a character. Splitting a
/// UTF-8 sequence must not make the line look like console-code-page bytes.
#[test]
fn a_utf8_character_split_across_chunks_is_reassembled() {
    let source = "信頼されていない\r\n".as_bytes().to_vec();
    let mut decoder = decoder();

    let mut decoded = String::new();
    for split in 0..source.len() {
        decoded.push_str(&decoder.push(&source[split..split + 1]));
    }

    assert_eq!(decoded, "信頼されていない\r\n");
}

#[test]
fn a_cp932_character_split_across_chunks_is_reassembled() {
    let source = cp932("日本語テスト\r\n");
    let mut decoder = decoder();

    let mut decoded = String::new();
    for split in 0..source.len() {
        decoded.push_str(&decoder.push(&source[split..split + 1]));
    }

    assert_eq!(decoded, "日本語テスト\r\n");
}

// ── streaming ──────────────────────────────────────────────────────────────

/// Output that never reaches a newline — a progress line, a prompt — must still
/// stream. Holding it back until a terminator would stall a long build's
/// display until it finished.
#[test]
fn an_unterminated_line_is_emitted_immediately() {
    let mut decoder = decoder();
    assert_eq!(decoder.push(b"Building "), "Building ");
    assert_eq!(decoder.push(b"[===>    ] 40%\r"), "[===>    ] 40%\r");
}

/// Truncated input at EOF is reported, not dropped: the bytes are all we will
/// ever get, so holding them back to wait for a completion loses them.
#[test]
fn finish_flushes_a_truncated_sequence() {
    let mut decoder = decoder();
    let truncated = &"信".as_bytes()[..2];
    assert_eq!(decoder.push(truncated), "", "an incomplete sequence waits for more input");
    assert!(!decoder.finish().is_empty(), "but at EOF it must surface rather than vanish");
}

#[test]
fn a_complete_trailing_line_does_not_wait_for_finish() {
    let mut decoder = decoder();
    assert_eq!(decoder.push(&cp932("最終行")), "最終行");
    assert_eq!(decoder.finish(), "", "push already emitted it");
}
