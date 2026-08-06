//! `--verbose` was declared in the CLI but wired to nothing, so it parsed
//! successfully and did absolutely nothing. These run the real binary because
//! that is the only way to prove the flag reaches the code that prints — a unit
//! test on the flag alone would have passed against the broken version too.

use std::process::Command;

/// A config pointing at a host that cannot resolve. The run fails, which is
/// fine: the diagnostics under test are printed before and during the attempt,
/// and failing fast keeps the test quick.
fn unreachable_config(dir: &std::path::Path) -> std::path::PathBuf {
    let path = dir.join("devsync.toml");
    std::fs::write(
        &path,
        r#"
[connection]
host = "devsync-no-such-host.invalid"
user = "nobody"
port = 22

[paths]
local_dir = "."
remote_dir = "C:\\work\\nowhere"

[commands]
build = "cargo build"
"#,
    )
    .unwrap();
    path
}

/// Returns (stdout, stderr). `args` is passed verbatim so tests can place the
/// flags wherever they want relative to the subcommand.
fn run(args: &[&str], config: &std::path::Path, dir: &std::path::Path) -> (String, String) {
    let output = Command::new(env!("CARGO_BIN_EXE_devsync"))
        .arg("--config")
        .arg(config)
        .args(args)
        .current_dir(dir)
        .output()
        .expect("failed to run devsync");
    assert!(
        !String::from_utf8_lossy(&output.stderr).contains("unexpected argument"),
        "devsync rejected the arguments outright, so this test would prove nothing: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    (
        String::from_utf8_lossy(&output.stdout).to_string(),
        String::from_utf8_lossy(&output.stderr).to_string(),
    )
}

#[test]
fn verbose_reports_the_ssh_command_it_spawns() {
    let dir = tempfile::tempdir().unwrap();
    let config = unreachable_config(dir.path());

    let (_stdout, stderr) = run(&["--verbose", "status"], &config, dir.path());

    assert!(
        stderr.contains("[devsync]"),
        "verbose diagnostics must be tagged so they are separable from the remote's \
         own stderr; got: {stderr}"
    );
    assert!(
        stderr.contains("spawning: ssh"),
        "the spawned ssh command line is the diagnostic worth having — it is what \
         a user reproduces by hand; got: {stderr}"
    );
    assert!(
        stderr.contains("devsync-no-such-host.invalid"),
        "the diagnostic must name the host actually used; got: {stderr}"
    );
}

#[test]
fn without_verbose_no_diagnostics_are_printed() {
    let dir = tempfile::tempdir().unwrap();
    let config = unreachable_config(dir.path());

    let (_stdout, stderr) = run(&["status"], &config, dir.path());

    assert!(
        !stderr.contains("[devsync]"),
        "diagnostics must stay off by default; got: {stderr}"
    );
}

/// `devsync status -v` is what people type. clap only accepts a top-level flag
/// after the subcommand when it is declared global, so this guards that.
#[test]
fn short_flag_works_after_the_subcommand() {
    let dir = tempfile::tempdir().unwrap();
    let config = unreachable_config(dir.path());

    let (_stdout, stderr) = run(&["status", "-v"], &config, dir.path());

    assert!(
        stderr.contains("spawning: ssh"),
        "-v must work on either side of the subcommand; got: {stderr}"
    );
}

/// Diagnostics belong on stderr: stdout carries the remote command's own output
/// during `exec`, and mixing the two would corrupt a piped build log.
#[test]
fn diagnostics_do_not_reach_stdout() {
    let dir = tempfile::tempdir().unwrap();
    let config = unreachable_config(dir.path());

    let (stdout, _stderr) = run(&["--verbose", "status"], &config, dir.path());

    assert!(!stdout.contains("[devsync]"), "diagnostics leaked to stdout: {stdout}");
}
