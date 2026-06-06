use std::fs;

#[path = "../src/config.rs"]
mod config;

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
        commands: config::CommandConfig::default(),
        sync: config::SyncConfig::default(),
    };

    assert!(cfg.command("build").unwrap_err().to_string().contains("commands.build"));
}
