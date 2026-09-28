//! The bundled `rainbow` and `simple` themes are installed into the config
//! directory as ordinary theme files the first time a config names them.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

const BIN: &str = env!("CARGO_BIN_EXE_superline");

const WIDGETS: &str = r#"[
    { "text": "text" }, { "cwd": { "max_length": 60, "wanted_seg_num": 5 } }, "username", { "time": { "format": "12:34" } },
    "read_only", "jobs", "shell", "cmd"
]"#;

fn scratch_dir(label: &str) -> PathBuf {
    let dir =
        std::env::temp_dir().join(format!("superline-bundled-{}-{label}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).expect("create scratch dir");
    dir
}

fn repo_theme(file: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("themes")
        .join(file)
}

fn write_config(dir: &Path, theme: &str) -> PathBuf {
    let path = dir.join("config.json");
    let theme = serde_json::to_string(theme).unwrap();
    fs::write(
        &path,
        format!(r#"{{ "theme": {theme}, "rows": [{{ "left": {WIDGETS} }}], "update": {{ "disable": true }} }}"#),
    )
    .expect("write config");
    path
}

fn run(command: &str, config: &Path) -> Output {
    let output = Command::new(BIN)
        .args([command, "fish", "-s", "0", "-c", "80", "--config"])
        .arg(config)
        .env("XDG_CACHE_HOME", config.parent().unwrap().join("cache"))
        .output()
        .expect("failed to run superline");
    assert!(output.status.success(), "{output:?}");
    output
}

/// The prompt drawn with the repo's copy of a bundled theme.
fn drawn_with_repo_theme(command: &str, file: &str) -> Vec<u8> {
    let dir = scratch_dir(&format!("{command}-reference-{file}"));
    let config = write_config(&dir, &repo_theme(file).to_string_lossy());
    let output = run(command, &config);
    let _ = fs::remove_dir_all(dir);
    output.stdout
}

#[test]
fn bundled_theme_names_resolve_with_or_without_json() {
    for (theme, file) in [
        ("rainbow", "rainbow.json"),
        ("rainbow.json", "rainbow.json"),
        ("simple", "simple.json"),
        ("simple.json", "simple.json"),
    ] {
        let dir = scratch_dir(&format!("resolve-{theme}"));
        let output = run("preview", &write_config(&dir, theme));
        assert_eq!(
            fs::read_to_string(dir.join(file)).unwrap(),
            fs::read_to_string(repo_theme(file)).unwrap(),
            "{theme} installs {file}"
        );
        assert_eq!(
            output.stdout,
            drawn_with_repo_theme("preview", file),
            "{theme}"
        );
        let _ = fs::remove_dir_all(dir);
    }
}

#[test]
fn an_existing_theme_file_is_left_untouched() {
    let dir = scratch_dir("existing");
    let edited = r#"{ "defaults": { "fg": 1, "bg": 2 }, "modules": {} }"#;
    fs::write(dir.join("rainbow.json"), edited).unwrap();
    let output = run("show", &write_config(&dir, "rainbow"));
    assert_eq!(
        fs::read_to_string(dir.join("rainbow.json")).unwrap(),
        edited
    );
    assert_ne!(output.stdout, drawn_with_repo_theme("show", "rainbow.json"));
    let _ = fs::remove_dir_all(dir);
}

#[test]
fn a_missing_theme_that_is_not_bundled_is_still_an_error() {
    let dir = scratch_dir("missing");
    let output = run("show", &write_config(&dir, "ocean"));
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("theme file not loaded"), "{stdout}");
    assert!(!dir.join("ocean.json").exists());
    let _ = fs::remove_dir_all(dir);
}

#[cfg(unix)]
#[test]
fn a_read_only_config_dir_still_draws_the_bundled_theme() {
    use std::os::unix::fs::PermissionsExt;

    let dir = scratch_dir("read-only");
    let config_dir = dir.join("config");
    fs::create_dir_all(&config_dir).unwrap();
    let config = write_config(&config_dir, "simple");
    fs::set_permissions(&config_dir, fs::Permissions::from_mode(0o555)).unwrap();
    let output = Command::new(BIN)
        .args(["show", "fish", "-s", "0", "-c", "80", "--config"])
        .arg(&config)
        .env("XDG_CACHE_HOME", dir.join("cache"))
        .output()
        .expect("failed to run superline");
    let installed = config_dir.join("simple.json").exists();
    fs::set_permissions(&config_dir, fs::Permissions::from_mode(0o755)).unwrap();

    assert!(output.status.success(), "{output:?}");
    assert!(!installed);
    assert_eq!(output.stdout, drawn_with_repo_theme("show", "simple.json"));
    let _ = fs::remove_dir_all(dir);
}
