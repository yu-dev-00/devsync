use crate::{exclude::ExcludeMatcher, manifest, path_safety, protocol::{self, Message}};
use anyhow::Result;
use std::collections::BTreeMap;
use std::io::{Read, Write};
use std::path::PathBuf;

#[derive(Debug, Clone)]
struct AgentConfig {
    remote_dir: PathBuf,
    #[allow(dead_code)]
    commands: BTreeMap<String, String>,
}

pub fn run_stdio_agent() -> Result<()> {
    let stdin = std::io::stdin();
    let stdout = std::io::stdout();
    run_agent(stdin.lock(), stdout.lock())
}

pub fn run_agent<R: Read, W: Write>(mut reader: R, mut writer: W) -> Result<()> {
    let mut config: Option<AgentConfig> = None;

    loop {
        let message = match protocol::read_message(&mut reader) {
            Ok(message) => message,
            Err(_) => break,
        };

        match message {
            Message::Config { remote_dir, commands } => {
                config = Some(AgentConfig { remote_dir: PathBuf::from(remote_dir), commands });
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
                let matcher = ExcludeMatcher::new(Vec::new())?;
                let manifest = manifest::build_manifest(&config.remote_dir, &matcher)?;
                protocol::write_message(&mut writer, &manifest.into())?;
            }
            Message::Hello { version } => {
                protocol::write_message(&mut writer, &Message::Hello { version })?;
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
                path_safety::validate_relative_path(&path)?;
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
                    path_safety::validate_relative_path(&path)?;
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
