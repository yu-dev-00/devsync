use crate::{
    client::RemoteClient,
    config::Config,
    diff,
    exclude::ExcludeMatcher,
    manifest::{self, Manifest},
    path_safety::{is_same_or_descendant, validate_relative_path},
    protocol::{self, Message},
};
use anyhow::{bail, Result};
use std::io::{Read, Write};
use std::path::Path;

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
    let started = std::time::Instant::now();
    let manifest = manifest::build_manifest(&config.paths.local_dir, &matcher)?;
    // The elapsed time is worth showing: it is how a cold walk (everything
    // hashed) is told apart from one served by the hash cache.
    crate::vlog!(
        "local manifest: {} file(s) in {:?} from {}",
        manifest.files.len(),
        started.elapsed(),
        config.paths.local_dir.display()
    );
    Ok(manifest)
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
    crate::vlog!("remote manifest: {} file(s)", remote_manifest.files.len());
    let plan = diff::calculate_diff(&local_manifest, &remote_manifest, delete);
    crate::vlog!(
        "plan: {} upload, {} delete, {} skip",
        plan.upload.len(),
        plan.delete.len(),
        plan.skipped
    );

    let (reader, writer) = client.streams();
    let (uploaded, deleted) = apply_plan(&config.paths.local_dir, &plan, reader, writer)?;
    println!("uploaded: {uploaded}");
    println!("deleted: {deleted}");
    println!("skipped: {}", plan.skipped);

    Ok(())
}

/// Apply a content diff over an established, configured agent connection.
pub fn apply_plan<R: Read, W: Write>(
    local_dir: &Path,
    plan: &diff::SyncPlan,
    reader: &mut R,
    writer: &mut W,
) -> Result<(usize, usize)> {
    // Only explicitly planned deletions may clear a file/directory collision.
    // Keep unrelated deletions after uploads, as before. Acknowledge the first
    // batch before sending bytes so neither stale errors nor counts leak across.
    for path in plan.upload.iter().chain(&plan.delete) {
        validate_relative_path(path)?;
    }
    let (blocking, remaining): (Vec<_>, Vec<_>) = plan.delete.iter().cloned().partition(|deleted| {
        plan.upload.iter().any(|uploaded| {
            is_same_or_descendant(deleted, uploaded) || is_same_or_descendant(uploaded, deleted)
        })
    });
    let mut deleted_before_upload = 0;
    if !blocking.is_empty() {
        let (_, deleted) = finish_batch(reader, writer, blocking)?;
        deleted_before_upload = deleted;
    }

    // NOTE (v1 limitation): file uploads are sent fire-and-forget; per-file
    // agent responses (e.g. a hash-mismatch Error) are not read here. Such an
    // Error is surfaced when the SyncComplete ack is read below, where it causes
    // the sync to bail. A future version should read per-file acknowledgements.
    for path in &plan.upload {
        let (message, bytes) = build_file_message(local_dir, path)?;
        crate::vlog!("uploading {path} ({} bytes)", bytes.len());
        protocol::write_message(writer, &message)?;
        writer.write_all(&bytes)?;
        writer.flush()?;
    }
    let (uploaded, deleted) = finish_batch(reader, writer, remaining)?;
    Ok((uploaded, deleted_before_upload + deleted))
}

fn finish_batch<R: Read, W: Write>(reader: &mut R, writer: &mut W, delete: Vec<String>) -> Result<(usize, usize)> {
    for path in &delete {
        crate::vlog!("deleting {path}");
    }

    protocol::write_message(writer, &Message::SyncPlan {
        upload: Vec::new(),
        delete,
    })?;

    // Wait for the agent to confirm it has processed all uploads and deletes
    // before the connection is dropped (Drop kills the ssh child). Reading the
    // SyncComplete ack guarantees the agent flushed every file write to disk.
    match protocol::read_message(reader)? {
        // Only the agent's own counts are echoed here. `skipped` is excluded on
        // purpose: it is decided locally, so the agent always reports 0 and
        // printing it would look like a discrepancy.
        Message::SyncComplete { uploaded, deleted, .. } => {
            crate::vlog!("agent acked: wrote {uploaded} file(s), deleted {deleted}");
            Ok((uploaded, deleted))
        }
        Message::Error { message } => bail!(message),
        other => bail!("unexpected response to sync_plan: {other:?}"),
    }
}

pub fn exec(config: &Config, name: &str) -> Result<i32> {
    let command = config.command(name)?;
    crate::vlog!("exec {name}: {command}");
    let started = std::time::Instant::now();
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
            Message::Exit { code } => {
                crate::vlog!("exec {name} exited {code} after {:?}", started.elapsed());
                return Ok(code);
            }
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
