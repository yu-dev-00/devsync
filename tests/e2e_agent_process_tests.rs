use std::collections::BTreeMap;
use std::io::Write;
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};

use devsync::protocol::{self, Message};

// ── helpers ────────────────────────────────────────────────────────────────

fn spawn_agent() -> (Child, ChildStdin, ChildStdout) {
    let mut child = Command::new(env!("CARGO_BIN_EXE_devsync"))
        .arg("agent")
        .arg("--stdio")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .expect("failed to spawn devsync agent --stdio");
    let stdin = child.stdin.take().unwrap();
    let stdout = child.stdout.take().unwrap();
    (child, stdin, stdout)
}

// ── test 1 ─────────────────────────────────────────────────────────────────

#[test]
fn e2e_handshake_then_sync_uploads_and_deletes() {
    let dir = tempfile::tempdir().unwrap();

    // Pre-create a stale file (will be deleted) and an excluded artifact (must survive).
    std::fs::write(dir.path().join("stale.txt"), "old").unwrap();
    std::fs::create_dir_all(dir.path().join("build")).unwrap();
    std::fs::write(dir.path().join("build").join("keep.bin"), "artifact").unwrap();

    let (mut child, mut stdin, mut stdout) = spawn_agent();

    // Handshake.
    devsync::client::perform_handshake(&mut stdout, &mut stdin)
        .expect("handshake must succeed");

    // Send Config with "build" in excludes.
    protocol::write_message(
        &mut stdin,
        &Message::Config {
            remote_dir: dir.path().to_string_lossy().to_string(),
            commands: BTreeMap::new(),
            exclude: vec!["build".to_string()],
        },
    )
    .unwrap();

    // Request manifest; verify excludes are applied remotely.
    protocol::write_message(&mut stdin, &Message::ManifestRequest).unwrap();
    let manifest_msg = protocol::read_message(&mut stdout).unwrap();
    let files = match manifest_msg {
        Message::Manifest { files } => files,
        other => panic!("expected Manifest, got {other:?}"),
    };
    let paths: Vec<&str> = files.iter().map(|f| f.path.as_str()).collect();
    assert!(paths.contains(&"stale.txt"), "stale.txt should appear in manifest; got {paths:?}");
    assert!(
        !paths.iter().any(|p| p.starts_with("build/")),
        "build/ must be excluded from manifest; got {paths:?}"
    );

    // Upload hello.txt.
    let hello_bytes = b"hi";
    protocol::write_message(
        &mut stdin,
        &Message::File {
            path: "hello.txt".into(),
            size: hello_bytes.len() as u64,
            hash: blake3::hash(hello_bytes).to_hex().to_string(),
        },
    )
    .unwrap();
    stdin.write_all(hello_bytes).unwrap();
    stdin.flush().unwrap();

    // Upload nested/dir/data.bin.
    let data_bytes = b"xyz";
    protocol::write_message(
        &mut stdin,
        &Message::File {
            path: "nested/dir/data.bin".into(),
            size: data_bytes.len() as u64,
            hash: blake3::hash(data_bytes).to_hex().to_string(),
        },
    )
    .unwrap();
    stdin.write_all(data_bytes).unwrap();
    stdin.flush().unwrap();

    // Send SyncPlan that deletes stale.txt (upload list empty — files were already sent above).
    protocol::write_message(
        &mut stdin,
        &Message::SyncPlan { upload: vec![], delete: vec!["stale.txt".into()] },
    )
    .unwrap();

    // Read SyncComplete.
    let reply = protocol::read_message(&mut stdout).unwrap();
    match reply {
        Message::SyncComplete { deleted, .. } => {
            assert_eq!(deleted, 1, "expected 1 deletion; got {deleted}");
        }
        other => panic!("expected SyncComplete, got {other:?}"),
    }

    // Verify on-disk state.
    assert_eq!(
        std::fs::read_to_string(dir.path().join("hello.txt")).unwrap(),
        "hi"
    );
    assert_eq!(
        std::fs::read_to_string(dir.path().join("nested").join("dir").join("data.bin")).unwrap(),
        "xyz"
    );
    assert!(!dir.path().join("stale.txt").exists(), "stale.txt should have been deleted");
    assert!(
        dir.path().join("build").join("keep.bin").exists(),
        "build/keep.bin must not be deleted (excluded)"
    );

    // Shut down.
    child.kill().ok();
    child.wait().ok();
}

// ── test 2 ─────────────────────────────────────────────────────────────────

#[test]
fn e2e_exec_runs_named_command_and_streams_exit() {
    let dir = tempfile::tempdir().unwrap();
    let (mut child, mut stdin, mut stdout) = spawn_agent();

    devsync::client::perform_handshake(&mut stdout, &mut stdin).expect("handshake");

    let mut commands = BTreeMap::new();
    commands.insert("build".to_string(), "Write-Output e2e-ok".to_string());
    protocol::write_message(
        &mut stdin,
        &Message::Config {
            remote_dir: dir.path().to_string_lossy().to_string(),
            commands,
            exclude: vec![],
        },
    )
    .unwrap();

    protocol::write_message(&mut stdin, &Message::Exec { name: "build".into() }).unwrap();

    // Collect Output frames until Exit.
    let mut stdout_data = String::new();
    let exit_code;
    loop {
        let msg = protocol::read_message(&mut stdout).unwrap();
        match msg {
            Message::Output { stream, data } if stream == "stdout" => {
                stdout_data.push_str(&data);
            }
            Message::Output { .. } => {} // stderr — ignore
            Message::Exit { code } => {
                exit_code = code;
                break;
            }
            other => panic!("unexpected message during exec: {other:?}"),
        }
    }

    assert!(
        stdout_data.contains("e2e-ok"),
        "stdout output must contain 'e2e-ok'; got: {stdout_data:?}"
    );
    assert_eq!(exit_code, 0, "PowerShell command must exit with code 0");

    child.kill().ok();
    child.wait().ok();
}

// ── test 2b ────────────────────────────────────────────────────────────────

/// Regression: PowerShell's `-Command` collapses every non-zero native exit
/// code to 1, so the agent must append an explicit `exit $LASTEXITCODE`.
/// Without it a `cargo build` failing with 101 is reported as 1, and the exact
/// code required by the spec's error-handling section is lost.
#[test]
fn e2e_exec_propagates_exact_nonzero_exit_code() {
    let dir = tempfile::tempdir().unwrap();
    let (mut child, mut stdin, mut stdout) = spawn_agent();

    devsync::client::perform_handshake(&mut stdout, &mut stdin).expect("handshake");

    let mut commands = BTreeMap::new();
    commands.insert("failing".to_string(), "cmd /c exit 7".to_string());
    protocol::write_message(
        &mut stdin,
        &Message::Config {
            remote_dir: dir.path().to_string_lossy().to_string(),
            commands,
            exclude: vec![],
        },
    )
    .unwrap();

    protocol::write_message(&mut stdin, &Message::Exec { name: "failing".into() }).unwrap();

    let exit_code = loop {
        match protocol::read_message(&mut stdout).unwrap() {
            Message::Output { .. } => {}
            Message::Exit { code } => break code,
            other => panic!("unexpected message during exec: {other:?}"),
        }
    };

    assert_eq!(exit_code, 7, "the exact remote exit code must survive, not collapse to 1");

    child.kill().ok();
    child.wait().ok();
}

// ── test 2c ────────────────────────────────────────────────────────────────

/// Output must reach the client while the command is still running, not be
/// buffered until it exits. The command prints, sleeps, then prints again; the
/// first Output frame has to arrive well before the sleep is over.
#[test]
fn e2e_exec_streams_output_before_the_command_exits() {
    let dir = tempfile::tempdir().unwrap();
    let (mut child, mut stdin, mut stdout) = spawn_agent();

    devsync::client::perform_handshake(&mut stdout, &mut stdin).expect("handshake");

    let mut commands = BTreeMap::new();
    commands.insert(
        "slow".to_string(),
        "Write-Output early; Start-Sleep -Seconds 3; Write-Output late".to_string(),
    );
    protocol::write_message(
        &mut stdin,
        &Message::Config {
            remote_dir: dir.path().to_string_lossy().to_string(),
            commands,
            exclude: vec![],
        },
    )
    .unwrap();

    let started = std::time::Instant::now();
    protocol::write_message(&mut stdin, &Message::Exec { name: "slow".into() }).unwrap();

    let mut first_output_at = None;
    let mut collected = String::new();
    let exit_code = loop {
        match protocol::read_message(&mut stdout).unwrap() {
            Message::Output { data, .. } => {
                if first_output_at.is_none() && data.contains("early") {
                    first_output_at = Some(started.elapsed());
                }
                collected.push_str(&data);
            }
            Message::Exit { code } => break code,
            other => panic!("unexpected message during exec: {other:?}"),
        }
    };

    let first = first_output_at.expect("the 'early' line must arrive as its own Output frame");
    assert!(
        first < std::time::Duration::from_millis(2500),
        "first output arrived after {first:?}; it must not wait for the 3s sleep to finish"
    );
    assert!(collected.contains("early") && collected.contains("late"), "got: {collected:?}");
    assert_eq!(exit_code, 0);

    child.kill().ok();
    child.wait().ok();
}

// ── test 2d ────────────────────────────────────────────────────────────────

/// Output larger than one read buffer is reassembled from several chunks, so
/// nothing may be dropped, duplicated, or reordered at the seams.
#[test]
fn e2e_exec_reassembles_output_larger_than_one_chunk() {
    let dir = tempfile::tempdir().unwrap();
    let (mut child, mut stdin, mut stdout) = spawn_agent();

    devsync::client::perform_handshake(&mut stdout, &mut stdin).expect("handshake");

    const LINES: usize = 2000; // ~20 KB, well past the 8 KB read buffer
    let mut commands = BTreeMap::new();
    commands.insert(
        "chatty".to_string(),
        format!("1..{LINES} | ForEach-Object {{ Write-Output \"line $_\" }}"),
    );
    protocol::write_message(
        &mut stdin,
        &Message::Config {
            remote_dir: dir.path().to_string_lossy().to_string(),
            commands,
            exclude: vec![],
        },
    )
    .unwrap();

    protocol::write_message(&mut stdin, &Message::Exec { name: "chatty".into() }).unwrap();

    let mut collected = String::new();
    loop {
        match protocol::read_message(&mut stdout).unwrap() {
            Message::Output { stream, data } if stream == "stdout" => collected.push_str(&data),
            Message::Output { .. } => {}
            Message::Exit { .. } => break,
            other => panic!("unexpected message during exec: {other:?}"),
        }
    }

    let lines: Vec<&str> = collected.lines().filter(|line| !line.trim().is_empty()).collect();
    assert_eq!(lines.len(), LINES, "every line must survive chunk reassembly");
    assert_eq!(lines[0].trim(), "line 1");
    assert_eq!(lines[LINES - 1].trim(), format!("line {LINES}"));

    child.kill().ok();
    child.wait().ok();
}

// ── test 3 ─────────────────────────────────────────────────────────────────

#[test]
fn e2e_unknown_exec_name_returns_error() {
    let dir = tempfile::tempdir().unwrap();
    let (mut child, mut stdin, mut stdout) = spawn_agent();

    devsync::client::perform_handshake(&mut stdout, &mut stdin).expect("handshake");

    protocol::write_message(
        &mut stdin,
        &Message::Config {
            remote_dir: dir.path().to_string_lossy().to_string(),
            commands: BTreeMap::new(),
            exclude: vec![],
        },
    )
    .unwrap();

    protocol::write_message(&mut stdin, &Message::Exec { name: "build".into() }).unwrap();

    let reply = protocol::read_message(&mut stdout).unwrap();
    match reply {
        Message::Error { message } => {
            assert!(
                message.contains("commands.build"),
                "error must reference commands.build; got: {message:?}"
            );
        }
        other => panic!("expected Error, got {other:?}"),
    }

    child.kill().ok();
    child.wait().ok();
}

// ── test 4 (arbitrary name) ────────────────────────────────────────────────

#[test]
fn e2e_exec_arbitrary_name() {
    let dir = tempfile::tempdir().unwrap();
    let (mut child, mut stdin, mut stdout) = spawn_agent();

    devsync::client::perform_handshake(&mut stdout, &mut stdin).expect("handshake");

    let mut commands = BTreeMap::new();
    commands.insert("lint".to_string(), "Write-Output lint-ok".to_string());
    protocol::write_message(
        &mut stdin,
        &Message::Config {
            remote_dir: dir.path().to_string_lossy().to_string(),
            commands,
            exclude: vec![],
        },
    )
    .unwrap();

    protocol::write_message(&mut stdin, &Message::Exec { name: "lint".into() }).unwrap();

    // Collect Output frames until Exit.
    let mut stdout_data = String::new();
    let exit_code;
    loop {
        let msg = protocol::read_message(&mut stdout).unwrap();
        match msg {
            Message::Output { stream, data } if stream == "stdout" => {
                stdout_data.push_str(&data);
            }
            Message::Output { .. } => {} // stderr — ignore
            Message::Exit { code } => {
                exit_code = code;
                break;
            }
            other => panic!("unexpected message during exec: {other:?}"),
        }
    }

    assert!(
        stdout_data.contains("lint-ok"),
        "stdout output must contain 'lint-ok'; got: {stdout_data:?}"
    );
    assert_eq!(exit_code, 0, "PowerShell command must exit with code 0");

    child.kill().ok();
    child.wait().ok();
}

// ── test 5 ─────────────────────────────────────────────────────────────────

#[test]
fn e2e_version_mismatch_is_rejected() {
    let (mut child, mut stdin, mut stdout) = spawn_agent();

    // Send a Hello with an unsupported version — bypass perform_handshake.
    protocol::write_message(&mut stdin, &Message::Hello { version: 999 }).unwrap();

    let reply = protocol::read_message(&mut stdout).unwrap();
    match reply {
        Message::Error { message } => {
            assert!(
                message.contains("version"),
                "error must mention 'version'; got: {message:?}"
            );
        }
        other => panic!("expected Error for version mismatch, got {other:?}"),
    }

    // Agent should have exited; wait for it to clean up.
    child.wait().ok();
}
