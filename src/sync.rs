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
