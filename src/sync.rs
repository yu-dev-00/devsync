use crate::{
    client::RemoteClient,
    config::Config,
    diff,
    exclude::ExcludeMatcher,
    manifest::{self, Manifest},
    protocol::Message,
};
use anyhow::{bail, Result};

/// Read a file under `local_dir` (relative slash path) and build a `File`
/// message whose `size` and `hash` are derived from the bytes actually read,
/// guaranteeing the header matches the payload even if the file changed since
/// the manifest was built.
pub fn build_file_message(local_dir: &std::path::Path, rel_path: &str) -> Result<(Message, Vec<u8>)> {
    let bytes = std::fs::read(local_dir.join(rel_path.replace('/', std::path::MAIN_SEPARATOR_STR)))?;
    let message = Message::File {
        path: rel_path.to_string(),
        size: bytes.len() as u64,
        hash: blake3::hash(&bytes).to_hex().to_string(),
    };
    Ok((message, bytes))
}

pub fn status(config: &Config) -> Result<()> {
    let local_manifest = local_manifest(config)?;
    let remote_manifest = remote_manifest(config)?;
    let plan = diff::calculate_diff(&local_manifest, &remote_manifest, true);

    println!("upload: {}", plan.upload.len());
    for path in &plan.upload {
        println!("  + {path}");
    }
    println!("delete: {}", plan.delete.len());
    for path in &plan.delete {
        println!("  - {path}");
    }
    println!("skipped: {}", plan.skipped);

    Ok(())
}

pub fn local_manifest(config: &Config) -> Result<Manifest> {
    let matcher = ExcludeMatcher::new(config.sync.exclude.clone())?;
    manifest::build_manifest(&config.paths.local_dir, &matcher)
}

pub fn remote_manifest(config: &Config) -> Result<Manifest> {
    let mut client = RemoteClient::connect(config)?;
    client.write(&Message::ManifestRequest)?;
    match client.read()? {
        Message::Manifest { files } => Ok(Manifest { files }),
        Message::Error { message } => bail!(message),
        other => bail!("unexpected response to manifest_request: {other:?}"),
    }
}

pub fn sync(config: &Config, delete: bool) -> Result<()> {
    let local_manifest = local_manifest(config)?;
    let mut client = RemoteClient::connect(config)?;
    client.write(&Message::ManifestRequest)?;
    let remote_manifest = match client.read()? {
        Message::Manifest { files } => Manifest { files },
        Message::Error { message } => bail!(message),
        other => bail!("unexpected response to manifest_request: {other:?}"),
    };
    let plan = diff::calculate_diff(&local_manifest, &remote_manifest, delete);

    // NOTE (v1 limitation): file uploads are sent fire-and-forget; per-file
    // agent responses (e.g. a hash-mismatch Error) are not read here. Such an
    // Error is surfaced when the SyncComplete ack is read below, where it causes
    // the sync to bail. A future version should read per-file acknowledgements.
    for path in &plan.upload {
        let (message, bytes) = build_file_message(&config.paths.local_dir, path)?;
        client.write(&message)?;
        client.raw_write_all(&bytes)?;
    }

    client.write(&Message::SyncPlan {
        upload: Vec::new(),
        delete: if delete { plan.delete.clone() } else { Vec::new() },
    })?;

    // Wait for the agent to confirm it has processed all uploads and deletes
    // before the connection is dropped (Drop kills the ssh child). Reading the
    // SyncComplete ack guarantees the agent flushed every file write to disk.
    match client.read()? {
        Message::SyncComplete { .. } => {}
        Message::Error { message } => bail!(message),
        other => bail!("unexpected response to sync_plan: {other:?}"),
    }

    println!("uploaded: {}", plan.upload.len());
    println!("deleted: {}", if delete { plan.delete.len() } else { 0 });
    println!("skipped: {}", plan.skipped);

    Ok(())
}

pub fn exec(config: &Config, name: &str) -> Result<i32> {
    config.command(name)?;
    let mut client = RemoteClient::connect(config)?;
    client.write(&Message::Exec { name: name.to_string() })?;

    loop {
        match client.read()? {
            Message::Output { stream, data } => {
                if stream == "stderr" {
                    eprint!("{data}");
                } else {
                    print!("{data}");
                }
            }
            Message::Exit { code } => return Ok(code),
            Message::Error { message } => bail!(message),
            other => bail!("unexpected response to exec: {other:?}"),
        }
    }
}

/// Execute a named remote command, synchronizing first unless `no_sync`.
pub fn run_command(config: &Config, name: &str, no_sync: bool) -> Result<i32> {
    config.command(name)?; // validate locally before any connection
    if !no_sync {
        sync(config, false)?;
    }
    exec(config, name)
}
