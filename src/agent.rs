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

/// Decode the longest valid UTF-8 prefix of `buffer`, removing what it consumed.
/// A trailing incomplete sequence is left in place for the next chunk; bytes
/// that can never be valid are replaced, matching `String::from_utf8_lossy`.
fn take_decodable(buffer: &mut Vec<u8>) -> String {
    let mut decoded = String::new();
    loop {
        match std::str::from_utf8(buffer) {
            Ok(text) => {
                decoded.push_str(text);
                buffer.clear();
                return decoded;
            }
            Err(error) => {
                let valid_up_to = error.valid_up_to();
                decoded.push_str(
                    std::str::from_utf8(&buffer[..valid_up_to]).expect("prefix is valid utf-8"),
                );
                match error.error_len() {
                    Some(invalid_len) => {
                        decoded.push('\u{FFFD}');
                        buffer.drain(..valid_up_to + invalid_len);
                    }
                    // Truncated at the chunk boundary — keep it for the next read.
                    None => {
                        buffer.drain(..valid_up_to);
                        return decoded;
                    }
                }
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
                protocol::write_message(&mut writer, &Message::SyncComplete {
                    uploaded: 0,
                    deleted,
                    skipped: 0,
                })?;
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

                // Chunk boundaries fall at arbitrary byte offsets, so a multi-byte UTF-8
                // sequence can straddle two reads. Buffer per stream and only decode the
                // complete prefix, or streaming would corrupt characters that the old
                // decode-everything-at-once path got right.
                let mut stdout_buf: Vec<u8> = Vec::new();
                let mut stderr_buf: Vec<u8> = Vec::new();
                for (stream, chunk) in receiver {
                    let buffer = if stream == "stderr" { &mut stderr_buf } else { &mut stdout_buf };
                    buffer.extend_from_slice(&chunk);
                    let data = take_decodable(buffer);
                    if !data.is_empty() {
                        protocol::write_message(
                            &mut writer,
                            &Message::Output { stream: stream.to_string(), data },
                        )?;
                    }
                }

                // At EOF a leftover is a genuinely truncated sequence, not a boundary.
                for (stream, buffer) in [("stdout", &stdout_buf), ("stderr", &stderr_buf)] {
                    if !buffer.is_empty() {
                        protocol::write_message(
                            &mut writer,
                            &Message::Output {
                                stream: stream.to_string(),
                                data: String::from_utf8_lossy(buffer).to_string(),
                            },
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
