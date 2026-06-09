use crate::{diff::SyncPlan, manifest::Manifest};
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::io::{Read, Write};

pub const PROTOCOL_VERSION: u32 = 3;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Message {
    Hello { version: u32 },
    HelloAck { agent_version: u32 },
    Config { remote_dir: String, commands: BTreeMap<String, String>, exclude: Vec<String> },
    ManifestRequest,
    Manifest { files: Vec<crate::manifest::ManifestEntry> },
    SyncPlan { upload: Vec<String>, delete: Vec<String> },
    File { path: String, size: u64, hash: String },
    SyncComplete { uploaded: usize, deleted: usize, skipped: usize },
    Exec { name: String },
    Output { stream: String, data: String },
    Exit { code: i32 },
    Error { message: String },
}

impl From<Manifest> for Message {
    fn from(value: Manifest) -> Self {
        Message::Manifest { files: value.files }
    }
}

impl From<SyncPlan> for Message {
    fn from(value: SyncPlan) -> Self {
        Message::SyncPlan { upload: value.upload, delete: value.delete }
    }
}

pub fn write_message<W: Write>(writer: &mut W, message: &Message) -> Result<()> {
    let json = serde_json::to_vec(message)?;
    let len = u32::try_from(json.len()).context("message is too large")?;
    writer.write_all(&len.to_be_bytes())?;
    writer.write_all(&json)?;
    writer.flush()?;
    Ok(())
}

pub fn read_message<R: Read>(reader: &mut R) -> Result<Message> {
    let mut len_buf = [0u8; 4];
    reader.read_exact(&mut len_buf)?;
    let len = u32::from_be_bytes(len_buf) as usize;
    let mut json = vec![0u8; len];
    reader.read_exact(&mut json)?;
    Ok(serde_json::from_slice(&json)?)
}
