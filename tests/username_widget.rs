//! Compatibility checks for the renamed Username widget.

use std::fs;
use std::path::PathBuf;
use std::process::Command;

const BIN: &str = env!("CARGO_BIN_EXE_superline");

fn scratch_dir(label: &str) -> PathBuf {
    let dir =
        std::env::temp_dir().join(format!("superline-username-{}-{label}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).expect("create scratch dir");
    dir
}

fn render(segment: &str) -> String {
    let root = scratch_dir(segment);
    let home = root.join("home");
    let cache = root.join("cache");
    let config_dir = home.join(".config/superline");
    fs::create_dir_all(&config_dir).expect("create config dir");
    fs::write(
        config_dir.join("config.json"),
        format!(
            r#"{{
                "theme": "rainbow",
                "update": {{ "disable": true }},
                "rows": [{{ "left": ["{segment}"] }}]
            }}"#
        ),
    )
    .expect("write username config");

    let output = Command::new(BIN)
        .args(["show", "fish", "-s", "0", "-c", "80"])
        .env("HOME", &home)
        .env("USERPROFILE", &home)
        .env("XDG_CACHE_HOME", &cache)
        .output()
        .expect("run superline");
    assert!(
        output.status.success(),
        "show failed for {segment}:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );

    let prompt = String::from_utf8_lossy(&output.stdout).into_owned();
    let _ = fs::remove_dir_all(&root);
    prompt
}

#[test]
fn user_and_username_configs_render_identically() {
    assert_eq!(render("user"), render("username"));
}
