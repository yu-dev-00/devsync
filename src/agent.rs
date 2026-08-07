use crate::{exclude::ExcludeMatcher, manifest, path_safety, protocol::{self, Message}};
use anyhow::Result;
use std::collections::BTreeMap;
use std::io::{Read, Write};
use std::path::PathBuf;

#[derive(Debug, Clone)]
struct AgentConfig {
    remote_dir: PathBuf,
    commands: BTreeMap<String, String>,
    exclude: Vec<String>,
}

/// Forward everything readable from one child pipe to `sender`, tagged with the
/// stream it came from. Ends at EOF, on a read error, or once the receiver is
/// gone. Read errors are swallowed deliberately: the exec loop still needs to
/// reap the child and report its exit code.
fn pump_stream<R: std::io::Read>(
    stream: &'static str,
    mut source: R,
    sender: std::sync::mpsc::Sender<(&'static str, Vec<u8>)>,
) {
    let mut chunk = [0u8; 8192];
    loop {
        match source.read(&mut chunk) {
            Ok(0) | Err(_) => break,
            Ok(read) => {
                if sender.send((stream, chunk[..read].to_vec())).is_err() {
                    break;
                }
            }
        }
    }
}

/// The encoding console programs write in on this machine. Windows PowerShell
/// encodes redirected stdout with the console output code page — CP932 on a
/// Japanese install, not UTF-8 — so decoding as UTF-8 turns every non-ASCII
/// character into U+FFFD, irreversibly.
#[cfg(windows)]
fn console_encoding() -> &'static encoding_rs::Encoding {
    // The agent runs under sshd with no console attached, where
    // GetConsoleOutputCP returns 0; the OEM code page is what console programs
    // fall back to for redirected output.
    let code_page = unsafe {
        match windows_sys::Win32::System::Console::GetConsoleOutputCP() {
            0 => windows_sys::Win32::Globalization::GetOEMCP(),
            attached => attached,
        }
    };
    u16::try_from(code_page)
        .ok()
        .and_then(codepage::to_encoding)
        .unwrap_or(encoding_rs::UTF_8)
}

#[cfg(not(windows))]
fn console_encoding() -> &'static encoding_rs::Encoding {
    encoding_rs::UTF_8
}

/// Longest byte sequence any console code page needs for one character. The
/// Windows DBCS code pages top out at two; the extra byte is slack.
const MAX_CONSOLE_SEQUENCE: usize = 3;

/// Decodes one output stream whose bytes may not all share an encoding.
///
/// Nothing forces the writers on a single pipe to agree. PowerShell encodes its
/// own output in the console code page (CP932 on a Japanese install), and so do
/// the classic console tools; anything built in Rust or Go — `uv`, `cargo`,
/// `rustc` — writes UTF-8 unconditionally and never consults that setting. Both
/// appeared in one run of one real project, so any single fixed encoding
/// garbles half the output: reading everything as UTF-8 destroyed PowerShell's
/// Japanese, and then reading everything as CP932 destroyed `uv`'s.
///
/// So the encoding is decided per line rather than configured: bytes that form
/// valid UTF-8 are UTF-8, anything else is the console code page. The test is
/// stable in the direction that matters, because the two readings of Shift-JIS
/// only re-align every six bytes — a whole message staying accidentally valid
/// UTF-8 by chance does not happen. Pure-ASCII lines decode identically either
/// way, so a coin-flip verdict there costs nothing.
///
/// Every segment handed to `decode` is self-contained, which is what lets each
/// one be judged on its own: a line ends at a terminator, which is never part
/// of a character, and the unterminated tail holds back any bytes that a later
/// chunk may complete. Nothing is carried in decoder state across a verdict.
pub struct OutputDecoder {
    console: &'static encoding_rs::Encoding,
    pending: Vec<u8>,
}

impl OutputDecoder {
    pub fn new(console: &'static encoding_rs::Encoding) -> Self {
        Self { console, pending: Vec::new() }
    }

    /// Append a chunk and decode everything decodable so far.
    pub fn push(&mut self, bytes: &[u8]) -> String {
        self.pending.extend_from_slice(bytes);
        let mut decoded = String::new();

        // One line at a time: a single chunk can carry a UTF-8 line from `uv` and
        // a console-code-page line from PowerShell back to back, and one verdict
        // taken over both would garble whichever lost. 0x0A and 0x0D never appear
        // inside a character in UTF-8 or in the DBCS console code pages, so this
        // split is safe before the encoding is known.
        while let Some(end) = self.pending.iter().position(|byte| *byte == b'\n' || *byte == b'\r') {
            let line: Vec<u8> = self.pending.drain(..=end).collect();
            decoded.push_str(&self.decode(&line));
        }

        // Emit the unterminated remainder too, rather than holding it back until a
        // newline that may be a whole build away — progress output that only
        // rewrites one line has to keep streaming. Only a trailing sequence that
        // the next chunk may complete stays behind.
        let complete = self.pending.len() - self.incomplete_tail();
        let tail: Vec<u8> = self.pending.drain(..complete).collect();
        decoded.push_str(&self.decode(&tail));

        decoded
    }

    /// Decode whatever is still held back. Anything left at EOF is truncated
    /// rather than merely split, so it is reported instead of silently dropped.
    pub fn finish(&mut self) -> String {
        let tail: Vec<u8> = std::mem::take(&mut self.pending);
        self.decode(&tail)
    }

    fn decode(&self, segment: &[u8]) -> String {
        match std::str::from_utf8(segment) {
            Ok(text) => text.to_string(),
            Err(_) => self.console.decode_without_bom_handling(segment).0.into_owned(),
        }
    }

    /// How many trailing bytes are a valid *prefix* of a character and might be
    /// completed by the next chunk. Zero when the tail is complete.
    fn incomplete_tail(&self) -> usize {
        match std::str::from_utf8(&self.pending) {
            // Complete as UTF-8, so `decode` will read it as UTF-8: nothing to wait for.
            Ok(_) => 0,
            // `error_len() == None` is precisely "ran out of input mid-sequence".
            Err(error) if error.error_len().is_none() => self.pending.len() - error.valid_up_to(),
            // Not UTF-8, so `decode` will read it in the console code page; ask how
            // much of the tail that encoding is still waiting on. encoding_rs has no
            // one-shot for this, so trim a byte at a time and see what stops the
            // complaint. Over-trimming a byte that is simply invalid costs nothing:
            // it is emitted with the next segment, or at `finish` regardless.
            Err(_) => {
                if !self.console.decode_without_bom_handling(&self.pending).1 {
                    return 0;
                }
                let cap = MAX_CONSOLE_SEQUENCE.min(self.pending.len());
                (1..=cap)
                    .find(|trimmed| {
                        let kept = &self.pending[..self.pending.len() - trimmed];
                        !self.console.decode_without_bom_handling(kept).1
                    })
                    .unwrap_or(0)
            }
        }
    }
}

pub fn run_stdio_agent() -> Result<()> {
    let stdin = std::io::stdin();
    let stdout = std::io::stdout();
    run_agent(stdin.lock(), stdout.lock())
}

pub fn run_agent<R: Read, W: Write>(mut reader: R, mut writer: W) -> Result<()> {
    // NOTE: the agent validates the Hello version when one is sent, but does not
    // *require* a Hello before operational messages. Cross-version safety still
    // holds in practice: an incompatible client's Config fails to deserialize
    // (the frame read errors and the loop ends), and the real client always
    // handshakes first and bails on mismatch. Enforcing handshake-first on the
    // agent (rejecting any pre-handshake operational message) is a v2 follow-up.
    let mut config: Option<AgentConfig> = None;
    // Files actually written since the last SyncComplete, so the ack reports what
    // the agent did rather than a placeholder. Reset when the ack is sent, which
    // is what bounds one sync from the next on a reused connection.
    let mut written_since_ack = 0usize;

    loop {
        let message = match protocol::read_message(&mut reader) {
            Ok(message) => message,
            Err(_) => break,
        };

        match message {
            Message::Config { remote_dir, commands, exclude } => {
                config = Some(AgentConfig { remote_dir: PathBuf::from(remote_dir), commands, exclude });
            }
            Message::ManifestRequest => {
                let Some(config) = &config else {
                    protocol::write_message(
                        &mut writer,
                        &Message::Error { message: "agent config has not been received".into() },
                    )?;
                    continue;
                };
                std::fs::create_dir_all(&config.remote_dir)?;
                let matcher = ExcludeMatcher::new(config.exclude.clone())?;
                let manifest = manifest::build_manifest(&config.remote_dir, &matcher)?;
                protocol::write_message(&mut writer, &manifest.into())?;
            }
            Message::Hello { version } => {
                if version == protocol::PROTOCOL_VERSION {
                    protocol::write_message(
                        &mut writer,
                        &Message::HelloAck { agent_version: protocol::PROTOCOL_VERSION },
                    )?;
                } else {
                    protocol::write_message(
                        &mut writer,
                        &Message::Error {
                            message: format!(
                                "unsupported protocol version: {version} (agent supports {})",
                                protocol::PROTOCOL_VERSION
                            ),
                        },
                    )?;
                    // Peer speaks an incompatible protocol version; stop serving this connection.
                    break;
                }
            }
            Message::File { path, size, hash } => {
                // Always consume the raw payload bytes first to keep the stream aligned,
                // regardless of whether config has been received.
                let mut bytes = vec![0u8; size as usize];
                reader.read_exact(&mut bytes)?;

                let Some(config) = &config else {
                    protocol::write_message(
                        &mut writer,
                        &Message::Error { message: "agent config has not been received".into() },
                    )?;
                    continue;
                };
                if let Err(error) = path_safety::validate_relative_path(&path) {
                    protocol::write_message(&mut writer, &Message::Error { message: error.to_string() })?;
                    continue;
                }
                let actual_hash = blake3::hash(&bytes).to_hex().to_string();
                if actual_hash != hash {
                    protocol::write_message(
                        &mut writer,
                        &Message::Error { message: format!("hash mismatch for {path}") },
                    )?;
                    continue;
                }
                let target = config.remote_dir.join(path.replace('/', std::path::MAIN_SEPARATOR_STR));
                if let Some(parent) = target.parent() {
                    std::fs::create_dir_all(parent)?;
                }
                std::fs::write(target, bytes)?;
                written_since_ack += 1;
            }
            Message::SyncPlan { upload: _, delete } => {
                let Some(config) = &config else {
                    protocol::write_message(
                        &mut writer,
                        &Message::Error { message: "agent config has not been received".into() },
                    )?;
                    continue;
                };
                let mut deleted = 0;
                for path in delete {
                    if let Err(error) = path_safety::validate_relative_path(&path) {
                        protocol::write_message(&mut writer, &Message::Error { message: error.to_string() })?;
                        continue;
                    }
                    let target = config.remote_dir.join(path.replace('/', std::path::MAIN_SEPARATOR_STR));
                    if target.is_file() {
                        std::fs::remove_file(target)?;
                        deleted += 1;
                    }
                }
                // `skipped` stays 0: skipping is a local decision made from the diff,
                // so the agent never learns about a file the client chose not to send.
                protocol::write_message(&mut writer, &Message::SyncComplete {
                    uploaded: written_since_ack,
                    deleted,
                    skipped: 0,
                })?;
                written_since_ack = 0;
            }
            Message::Exec { name } => {
                let Some(config) = &config else {
                    protocol::write_message(
                        &mut writer,
                        &Message::Error { message: "agent config has not been received".into() },
                    )?;
                    continue;
                };
                let Some(command) = config.commands.get(&name) else {
                    protocol::write_message(
                        &mut writer,
                        &Message::Error { message: format!("commands.{name} is not defined") },
                    )?;
                    continue;
                };

                // PowerShell's `-Command` does not forward a native child process's exit
                // code: every non-zero result collapses to 1, so `cargo build` failing
                // with 101 (or a script calling `exit 3`) would be reported as 1. An
                // explicit trailing `exit $LASTEXITCODE` propagates the real code. When
                // the command ran no native process $LASTEXITCODE is $null, and
                // `exit $null` yields 0, so successful runs are unaffected.
                let wrapped_command = format!("{command}; exit $LASTEXITCODE");

                // stdin MUST be null: the agent's own stdin is the protocol stream, and
                // an inherited handle would let the child consume frames meant for us.
                let mut child = std::process::Command::new("powershell")
                    .arg("-NoProfile")
                    .arg("-ExecutionPolicy")
                    .arg("Bypass")
                    .arg("-Command")
                    .arg(&wrapped_command)
                    .current_dir(&config.remote_dir)
                    .stdin(std::process::Stdio::null())
                    .stdout(std::process::Stdio::piped())
                    .stderr(std::process::Stdio::piped())
                    .spawn()?;

                let child_stdout = child.stdout.take().expect("stdout was piped");
                let child_stderr = child.stderr.take().expect("stderr was piped");

                // Both pipes must be drained concurrently: a child that fills one while
                // we block on the other would deadlock. Reader threads forward raw bytes
                // to this thread, which owns `writer` — protocol frames must never be
                // written from two threads or they would interleave mid-frame.
                let (sender, receiver) = std::sync::mpsc::channel::<(&'static str, Vec<u8>)>();
                let stdout_pump = {
                    let sender = sender.clone();
                    std::thread::spawn(move || pump_stream("stdout", child_stdout, sender))
                };
                let stderr_pump = std::thread::spawn(move || pump_stream("stderr", child_stderr, sender));

                // Chunk boundaries fall at arbitrary byte offsets, so a multi-byte
                // sequence can straddle two reads. Each stream keeps its own decoder,
                // which holds the partial sequence until the next chunk completes it.
                let encoding = console_encoding();
                let mut stdout_decoder = OutputDecoder::new(encoding);
                let mut stderr_decoder = OutputDecoder::new(encoding);

                for (stream, chunk) in receiver {
                    let decoder = if stream == "stderr" {
                        &mut stderr_decoder
                    } else {
                        &mut stdout_decoder
                    };
                    let data = decoder.push(&chunk);
                    if !data.is_empty() {
                        protocol::write_message(
                            &mut writer,
                            &Message::Output { stream: stream.to_string(), data },
                        )?;
                    }
                }

                // Flush each decoder: anything still held back is truncated rather than
                // merely split, and must be reported instead of silently dropped.
                for (stream, decoder) in
                    [("stdout", &mut stdout_decoder), ("stderr", &mut stderr_decoder)]
                {
                    let data = decoder.finish();
                    if !data.is_empty() {
                        protocol::write_message(
                            &mut writer,
                            &Message::Output { stream: stream.to_string(), data },
                        )?;
                    }
                }

                let _ = stdout_pump.join();
                let _ = stderr_pump.join();
                let status = child.wait()?;
                protocol::write_message(&mut writer, &Message::Exit {
                    code: status.code().unwrap_or(1),
                })?;
            }
            _ => {
                protocol::write_message(
                    &mut writer,
                    &Message::Error { message: "unsupported agent message".into() },
                )?;
            }
        }
    }

    Ok(())
}
