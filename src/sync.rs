use crate::{
    client::RemoteClient,
    config::Config,
    diff,
    exclude::ExcludeMatcher,
    manifest::{self, Manifest},
    protocol::Message,
};
use anyhow::{bail, Result};

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

    for path in &plan.upload {
        let entry = local_manifest
            .files
            .iter()
            .find(|entry| &entry.path == path)
            .ok_or_else(|| anyhow::anyhow!("missing local manifest entry for {path}"))?;
        let bytes = std::fs::read(
            config.paths.local_dir.join(path.replace('/', std::path::MAIN_SEPARATOR_STR)),
        )?;
        client.write(&Message::File {
            path: entry.path.clone(),
            size: entry.size,
            hash: entry.hash.clone(),
        })?;
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
