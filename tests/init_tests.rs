use devsync::{config::Config, init};
use std::fs;

fn options() -> init::InitOptions {
    init::InitOptions::default()
}

/// The whole point of init is producing a config devsync can actually read, so
/// the generated file must survive a real load, not merely look plausible.
#[test]
fn generated_config_loads() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("devsync.toml");

    init::run(&path, &options()).unwrap();

    let loaded = Config::load(&path).unwrap();
    assert_eq!(loaded.connection.port, 22);
    assert_eq!(loaded.paths.local_dir.to_string_lossy(), ".");
}

#[test]
fn flags_are_substituted_into_the_template() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("devsync.toml");

    init::run(
        &path,
        &init::InitOptions {
            host: Some("build-box".into()),
            user: Some("alice".into()),
            remote_dir: Some(r"D:\work\app".into()),
            ..options()
        },
    )
    .unwrap();

    let loaded = Config::load(&path).unwrap();
    assert_eq!(loaded.connection.host, "build-box");
    assert_eq!(loaded.connection.user, "alice");
    // Windows paths are the reason escaping matters: an unescaped C:\work would
    // make \w an invalid TOML escape and the load above would have failed.
    assert_eq!(loaded.paths.remote_dir, r"D:\work\app");
}

/// A config holds connection details someone typed. Silently replacing it would
/// be a bad trade for the convenience of not passing a flag.
#[test]
fn refuses_to_overwrite_without_force() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("devsync.toml");
    fs::write(&path, "# hand written\n").unwrap();

    let error = init::run(&path, &options()).unwrap_err().to_string();

    assert!(error.contains("--force"), "the error should say how to proceed; got: {error}");
    assert_eq!(fs::read_to_string(&path).unwrap(), "# hand written\n");
}

/// Refreshing the skill after an upgrade must not cost you your config. If this
/// errored, the only documented way through would be --force, which takes the
/// connection details and [commands] with it.
#[test]
fn install_skill_leaves_an_existing_config_alone() {
    let dir = tempfile::tempdir().unwrap();
    let home = tempfile::tempdir().unwrap();
    let path = dir.path().join("devsync.toml");
    fs::write(&path, "# hand written\n").unwrap();

    std::env::set_var("USERPROFILE", home.path());
    let result = init::run(&path, &init::InitOptions { install_skill: true, ..options() });

    assert!(result.is_ok(), "installing the skill must not fail on an existing config");
    assert_eq!(fs::read_to_string(&path).unwrap(), "# hand written\n");
    assert!(home.path().join(".claude/skills/devsync/SKILL.md").is_file());
}

#[test]
fn force_overwrites() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("devsync.toml");
    fs::write(&path, "# hand written\n").unwrap();

    init::run(&path, &init::InitOptions { force: true, ..options() }).unwrap();

    assert!(Config::load(&path).is_ok());
}

#[test]
fn gitignore_gains_the_cache_entry_once() {
    let dir = tempfile::tempdir().unwrap();
    fs::create_dir(dir.path().join(".git")).unwrap();
    fs::write(dir.path().join(".gitignore"), "target/\n").unwrap();
    let path = dir.path().join("devsync.toml");

    init::run(&path, &options()).unwrap();
    init::run(&path, &init::InitOptions { force: true, ..options() }).unwrap();

    let gitignore = fs::read_to_string(dir.path().join(".gitignore")).unwrap();
    assert!(gitignore.contains("target/"), "existing entries must survive");
    assert_eq!(
        gitignore.matches(".devsync").count(),
        1,
        "re-running init must not keep appending; got: {gitignore:?}"
    );
}

#[test]
fn gitignore_is_created_when_missing_in_a_repository() {
    let dir = tempfile::tempdir().unwrap();
    fs::create_dir(dir.path().join(".git")).unwrap();

    init::run(&dir.path().join("devsync.toml"), &options()).unwrap();

    let gitignore = fs::read_to_string(dir.path().join(".gitignore")).unwrap();
    assert!(gitignore.contains(".devsync"));
}

/// Outside a repository a .gitignore would be litter in a directory that has
/// nothing to do with git.
#[test]
fn no_gitignore_outside_a_repository() {
    let dir = tempfile::tempdir().unwrap();

    init::run(&dir.path().join("devsync.toml"), &options()).unwrap();

    assert!(!dir.path().join(".gitignore").exists());
}

#[test]
fn skill_installs_under_the_home_directory() {
    let home = tempfile::tempdir().unwrap();

    let installed = init::install_skill(home.path()).unwrap();

    assert_eq!(installed, home.path().join(".claude/skills/devsync/SKILL.md"));
    let contents = fs::read_to_string(&installed).unwrap();
    assert!(contents.starts_with("---"), "a skill needs its YAML frontmatter");
    assert!(contents.contains("name: devsync"));
}

/// Re-running after an upgrade is how the skill is refreshed, so a stale copy
/// must be replaced rather than left in place.
#[test]
fn installing_the_skill_refreshes_an_existing_copy() {
    let home = tempfile::tempdir().unwrap();
    let installed = init::install_skill(home.path()).unwrap();
    fs::write(&installed, "stale content").unwrap();

    init::install_skill(home.path()).unwrap();

    assert!(fs::read_to_string(&installed).unwrap().contains("name: devsync"));
}
