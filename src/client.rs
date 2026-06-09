use crate::{config::Config, protocol::{self, Message}};
use anyhow::{Context, Result};
use std::collections::BTreeMap;
use std::io::{BufReader, BufWriter, Read, Write};
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};

/// Perform the protocol handshake: send our Hello, then require the peer to
/// reply with a Hello at the same protocol version. Returns Err if the peer
/// reports an error or a different version.
pub fn perform_handshake<R: Read, W: Write>(reader: &mut R, writer: &mut W) -> Result<()> {
    protocol::write_message(writer, &Message::Hello { version: protocol::PROTOCOL_VERSION })?;
    match protocol::read_message(reader)? {
        Message::Hello { version } if version == protocol::PROTOCOL_VERSION => Ok(()),
        Message::Hello { version } => {
            anyhow::bail!("remote agent protocol version {version} != local {}", protocol::PROTOCOL_VERSION)
        }
        Message::Error { message } => anyhow::bail!(message),
        other => anyhow::bail!("unexpected handshake response: {other:?}"),
    }
}

pub struct RemoteClient {
    child: Child,
    reader: BufReader<ChildStdout>,
    writer: BufWriter<ChildStdin>,
}

impl RemoteClient {
    pub fn connect(config: &Config) -> Result<Self> {
        let target = format!("{}@{}", config.connection.user, config.connection.host);
        let remote_command = format!("\"{}\" agent --stdio", config.connection.agent_path);
        let mut child = Command::new("ssh")
            .arg("-p")
            .arg(config.connection.port.to_string())
            .arg(target)
            .arg(remote_command)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn()
            .context("failed to start ssh.exe")?;

        let stdin = child.stdin.take().context("failed to open ssh stdin")?;
        let stdout = child.stdout.take().context("failed to open ssh stdout")?;

        let mut client = Self {
            child,
            reader: BufReader::new(stdout),
            writer: BufWriter::new(stdin),
        };
        perform_handshake(&mut client.reader, &mut client.writer)?;
        client.send_config(config)?;
        Ok(client)
    }

    fn send_config(&mut self, config: &Config) -> Result<()> {
        let mut commands = BTreeMap::new();
        if let Some(value) = &config.commands.build {
            commands.insert("build".to_string(), value.clone());
        }
        if let Some(value) = &config.commands.run {
            commands.insert("run".to_string(), value.clone());
        }
        if let Some(value) = &config.commands.test {
            commands.insert("test".to_string(), value.clone());
        }
        self.write(&Message::Config {
            remote_dir: config.paths.remote_dir.clone(),
            commands,
            exclude: config.sync.exclude.clone(),
        })
    }

    pub fn write(&mut self, message: &Message) -> Result<()> {
        protocol::write_message(&mut self.writer, message)
    }

    pub fn raw_write_all(&mut self, bytes: &[u8]) -> Result<()> {
        use std::io::Write;
        self.writer.write_all(bytes)?;
        self.writer.flush()?;
        Ok(())
    }

    pub fn read(&mut self) -> Result<Message> {
        protocol::read_message(&mut self.reader)
    }
}

impl Drop for RemoteClient {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}
