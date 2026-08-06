use crate::{config::Config, protocol::{self, Message}};
use anyhow::{Context, Result};
use std::io::{BufReader, BufWriter, Read, Write};
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};

/// Perform the protocol handshake: send our Hello, then require the peer to
/// reply with a HelloAck carrying the same protocol version. A plain Hello
/// reply (which an older echo-only agent would produce) is rejected, as is an
/// Error or any other message.
pub fn perform_handshake<R: Read, W: Write>(reader: &mut R, writer: &mut W) -> Result<()> {
    protocol::write_message(writer, &Message::Hello { version: protocol::PROTOCOL_VERSION })?;
    // A failed read here means the agent never answered — almost always a
    // transport or deployment problem rather than a protocol one. The raw io
    // error ("failed to fill whole buffer") is useless on its own, so name the
    // two things worth checking. ssh.exe's own stderr is inherited and usually
    // prints the underlying cause just above this message.
    crate::vlog!("handshake: sent hello version {}", protocol::PROTOCOL_VERSION);
    let response = protocol::read_message(reader).context(
        "no response from the remote agent: check that ssh can reach the host and \
         that connection.agent_path points at devsync.exe on the remote machine",
    )?;
    match response {
        Message::HelloAck { agent_version } if agent_version == protocol::PROTOCOL_VERSION => {
            crate::vlog!("handshake: agent acknowledged version {agent_version}");
            Ok(())
        }
        Message::HelloAck { agent_version } => {
            anyhow::bail!(
                "remote agent protocol version {agent_version} != local {}",
                protocol::PROTOCOL_VERSION
            )
        }
        Message::Error { message } => anyhow::bail!(message),
        other => anyhow::bail!(
            "unexpected handshake response (remote agent may be an incompatible older version): {other:?}"
        ),
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
        // Printed before spawning, so it is visible even when ssh itself fails to
        // start. This line is the one worth copying into a terminal by hand when
        // a connection problem needs isolating from devsync.
        crate::vlog!(
            "spawning: ssh -p {} {target} {remote_command}",
            config.connection.port
        );
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
        crate::vlog!(
            "sending config: remote_dir={}, {} command(s), {} exclude(s)",
            config.paths.remote_dir,
            config.commands.len(),
            config.sync.exclude.len()
        );
        self.write(&Message::Config {
            remote_dir: config.paths.remote_dir.clone(),
            commands: config.commands.clone(),
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
