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
         that connection.agent_path points at devsync on the remote machine",
    )?;
    match response {
        Message::HelloAck { agent_version } if agent_version == protocol::PROTOCOL_VERSION => {
            crate::vlog!("handshake: agent acknowledged version {agent_version}");
            Ok(())
        }
        Message::HelloAck { agent_version } => {
            anyhow::bail!(
                "protocol version mismatch: remote agent speaks {agent_version}, local devsync speaks {}; \
                 upgrade the {} side so both run the same devsync build",
                protocol::PROTOCOL_VERSION,
                if agent_version < protocol::PROTOCOL_VERSION { "remote" } else { "local" },
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
            .context("could not run ssh: install the OpenSSH client or put ssh on PATH")?;

        let stdin = child.stdin.take().context("failed to open ssh stdin")?;
        let stdout = child.stdout.take().context("failed to open ssh stdout")?;

        let mut client = Self {
            child,
            reader: BufReader::new(stdout),
            writer: BufWriter::new(stdin),
        };
        if let Err(error) = perform_handshake(&mut client.reader, &mut client.writer) {
            // An answered handshake (version mismatch, Error frame) explains itself.
            // Silence means ssh or the agent died, and ssh's exit status says which.
            if error.root_cause().downcast_ref::<std::io::Error>().is_none() {
                return Err(error);
            }
            return Err(client.explain_silent_agent(config));
        }
        client.send_config(config)?;
        Ok(client)
    }

    fn explain_silent_agent(&mut self, config: &Config) -> anyhow::Error {
        let c = &config.connection;
        // The stream already hit EOF, so ssh is exiting; bound the wait anyway.
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(3);
        let status = loop {
            match self.child.try_wait() {
                Ok(Some(status)) => break status.code(),
                Ok(None) if std::time::Instant::now() < deadline => {
                    std::thread::sleep(std::time::Duration::from_millis(50))
                }
                _ => break None,
            }
        };
        crate::vlog!("ssh exited with {status:?}");
        // 255 is ssh's own failure code; anything else came from the remote side.
        if status == Some(255) {
            anyhow::anyhow!(
                "ssh could not connect to {}@{} port {} (ssh's own error is printed above). \
                 Try: ssh -p {} {}@{} \"echo ok\"",
                c.user, c.host, c.port, c.port, c.user, c.host
            )
        } else {
            anyhow::anyhow!(
                "ssh connected, but the remote agent did not answer (ssh exited {}). \
                 Check connection.agent_path (now \"{}\"): it must name devsync on the remote, \
                 e.g. \"/home/<user>/.local/bin/devsync\" on Linux. Run with -v and try the \
                 printed ssh command by hand.",
                status.map_or("without a code".to_string(), |code| format!("with {code}")),
                c.agent_path
            )
        }
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
            .context("the remote agent closed the connection; its own error, if any, is printed above")
    }

    pub(crate) fn streams(&mut self) -> (&mut impl Read, &mut impl Write) {
        (&mut self.reader, &mut self.writer)
    }
}

impl Drop for RemoteClient {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}
