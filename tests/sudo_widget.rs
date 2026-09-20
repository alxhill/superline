//! End-to-end checks for the cached Sudo widget.

use std::fs;
use std::path::PathBuf;
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

const BIN: &str = env!("CARGO_BIN_EXE_superline");
const SUDO_SYMBOL: &str = "⚿";

fn scratch_dir(label: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("superline-sudo-{}-{label}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).expect("create scratch dir");
    dir
}

fn render(cached: bool) -> String {
    let root = scratch_dir(&format!("render-{cached}"));
    let home = root.join("home");
    let cache = root.join("cache");
    let config_dir = home.join(".config/superline");
    let cache_dir = cache.join("superline");
    fs::create_dir_all(&config_dir).expect("create config dir");
    fs::create_dir_all(&cache_dir).expect("create cache dir");
    fs::write(
        config_dir.join("config.json"),
        r#"{
            "theme": "rainbow",
            "update": { "disable": true },
            "rows": [{ "left": ["sudo"] }]
        }"#,
    )
    .expect("write sudo config");

    let fetched_at = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system clock after epoch")
        .as_secs();
    fs::write(
        cache_dir.join("sudo-default.json"),
        format!(r#"{{"fetched_at":{fetched_at},"value":{cached}}}"#),
    )
    .expect("write sudo cache");

    let output = Command::new(BIN)
        .args(["show", "fish", "-s", "0", "-c", "80"])
        .env("HOME", &home)
        .env("USERPROFILE", &home)
        .env("XDG_CACHE_HOME", &cache)
        .output()
        .expect("run superline");
    assert!(
        output.status.success(),
        "show failed:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );

    let prompt = String::from_utf8_lossy(&output.stdout).into_owned();
    let _ = fs::remove_dir_all(&root);
    prompt
}

#[test]
fn cached_credentials_show_the_marker() {
    assert!(render(true).contains(SUDO_SYMBOL));
}

#[test]
fn uncached_credentials_hide_the_marker() {
    assert!(!render(false).contains(SUDO_SYMBOL));
}
