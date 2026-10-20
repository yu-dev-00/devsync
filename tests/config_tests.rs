use std::fs;

use devsync::config;

#[test]
fn hidden_config_resolves_local_paths_against_project_root() {
    let project = tempfile::tempdir().unwrap();
    fs::create_dir(project.path().join(".devsync")).unwrap();
    let path = project.path().join(".devsync/config.toml");
    for local in [".", "source"] {
        fs::write(&path, format!("[connection]\nhost='box'\nuser='alice'\n[paths]\nlocal_dir='{local}'\nremote_dir='C:/work/app'\n")).unwrap();
        let cfg = config::Config::load(&path).unwrap();
        assert_eq!(cfg.paths.local_dir, project.path().join(local));
    }
}

#[test]
fn loads_defaults_and_required_fields() {
    let dir = tempfile::tempdir().unwrap();
    let config_path = dir.path().join("devsync.toml");
    fs::write(
        &config_path,
        r#"
[connection]
host = "remote-pc"
user = "alice"

[paths]
remote_dir = "C:\\work\\project"
"#,
    )
    .unwrap();

    let cfg = config::Config::load(&config_path).unwrap();

    assert_eq!(cfg.connection.host, "remote-pc");
    assert_eq!(cfg.connection.user, "alice");
    assert_eq!(cfg.connection.port, 22);
    assert_eq!(cfg.connection.agent_path, "devsync.exe");
    assert_eq!(cfg.paths.local_dir.to_string_lossy(), ".");
    assert_eq!(cfg.paths.remote_dir, r"C:\work\project");
}

#[test]
fn rejects_missing_required_fields() {
    let dir = tempfile::tempdir().unwrap();
    let config_path = dir.path().join("devsync.toml");
    fs::write(&config_path, "[connection]\nhost = \"remote-pc\"\n").unwrap();

    let error = config::Config::load(&config_path).unwrap_err().to_string();

    assert!(error.contains("connection.user"));
    assert!(error.contains("paths.remote_dir"));
}

#[test]
fn command_is_required_only_when_requested() {
    let cfg = config::Config {
        connection: config::ConnectionConfig {
            host: "remote-pc".to_string(),
            user: "alice".to_string(),
            port: 22,
            agent_path: "devsync.exe".to_string(),
        },
        paths: config::PathConfig {
            local_dir: ".".into(),
            remote_dir: r"C:\work\project".to_string(),
        },
        commands: std::collections::BTreeMap::new(),
        sync: config::SyncConfig::default(),
    };

    assert!(cfg.command("build").unwrap_err().to_string().contains("commands.build"));
}

#[test]
fn arbitrary_command_names_load_from_toml() {
    let dir = tempfile::tempdir().unwrap();
    let config_path = dir.path().join("devsync.toml");
    fs::write(
        &config_path,
        r#"
[connection]
host = "remote-pc"
user = "alice"

[paths]
remote_dir = "C:\\work\\project"

[commands]
lint = "echo lint-ok"
build = "echo build"
"#,
    )
    .unwrap();

    let cfg = config::Config::load(&config_path).unwrap();

    assert_eq!(cfg.command("lint").unwrap(), "echo lint-ok");
    assert_eq!(cfg.command("build").unwrap(), "echo build");

    let err = cfg.command("missing").unwrap_err().to_string();
    assert!(err.contains("commands.missing"), "error was: {err}");
}

#[test]
fn load_errors_point_at_the_fix() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("devsync.toml");
    let err = |path: &std::path::Path| format!("{:#}", config::Config::load(path).unwrap_err());

    assert!(err(&path).contains("devsync init"), "missing config: {}", err(&path));

    // A misspelled section is reported where it is, not as a missing field.
    fs::write(&path, "[connection]\nhost='box'\nuser='alice'\n[path]\nremote_dir='C:/work/app'\n").unwrap();
    assert!(err(&path).contains("unknown field `path`"), "typo: {}", err(&path));

    fs::write(&path, "[connection]\nhost='box'\nuser='alice'\n").unwrap();
    let missing = err(&path);
    assert!(missing.contains("paths.remote_dir") && missing.contains("devsync.toml"), "missing field: {missing}");
}

#[test]
fn undefined_command_lists_the_defined_ones() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("devsync.toml");
    fs::write(&path, "[connection]\nhost='box'\nuser='alice'\n[paths]\nremote_dir='C:/work/app'\n[commands]\nbuild='make'\ntest='make test'\n").unwrap();
    let cfg = config::Config::load(&path).unwrap();
    assert_eq!(cfg.command("bulid").unwrap_err().to_string(), "commands.bulid is not defined; defined: build, test");
}
