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

                // NOTE (v1 limitation): `.output()` buffers all stdout/stderr until the
                // process exits, so a long-running command shows no output until it
                // finishes. The protocol and client exec loop already support incremental
                // Output frames; a future version should `.spawn()` and stream output via
                // reader threads. PowerShell output is decoded lossily below.
                let output = std::process::Command::new("powershell")
                    .arg("-NoProfile")
                    .arg("-ExecutionPolicy")
                    .arg("Bypass")
                    .arg("-Command")
                    .arg(command)
                    .current_dir(&config.remote_dir)
                    .output()?;

                if !output.stdout.is_empty() {
                    protocol::write_message(&mut writer, &Message::Output {
                        stream: "stdout".into(),
                        data: String::from_utf8_lossy(&output.stdout).to_string(),
                    })?;
                }
                if !output.stderr.is_empty() {
                    protocol::write_message(&mut writer, &Message::Output {
                        stream: "stderr".into(),
                        data: String::from_utf8_lossy(&output.stderr).to_string(),
                    })?;
                }
                protocol::write_message(&mut writer, &Message::Exit {
                    code: output.status.code().unwrap_or(1),
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
