//! End-to-end checks for the cached NATS context widget.

use std::fs;
use std::path::PathBuf;
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

use superline::cache::{hash_id, Source};
use superline::modules::NatsLookup;

const BIN: &str = env!("CARGO_BIN_EXE_superline");
const NATS_ICON: &str = "✉️ ";

fn scratch_dir(label: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("superline-nats-{}-{label}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).expect("create scratch dir");
    dir
}

#[test]
fn cached_context_renders_with_the_default_marker() {
    let root = scratch_dir("cached");
    let home = root.join("home");
    let cache = root.join("cache");
    let bin_dir = root.join("bin");
    let config_dir = home.join(".config/superline");
    fs::create_dir_all(&config_dir).expect("create config directory");
    fs::create_dir_all(&bin_dir).expect("create bin directory");

    // resolve_binary only needs to find a regular file when a fresh cache is
    // absent. A current cache lets this rendering test avoid spawning it.
    let nats_binary = bin_dir.join(if cfg!(windows) { "nats.exe" } else { "nats" });
    fs::write(&nats_binary, b"stub").expect("write nats stub");

    let lookup = NatsLookup {
        binary: nats_binary,
    };
    let cache_dir = cache.join("superline");
    fs::create_dir_all(&cache_dir).expect("create cache directory");
    let fetched_at = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("current time")
        .as_secs();
    fs::write(
        cache_dir.join(format!("nats-{}.json", hash_id(&lookup.binary))),
        format!(r#"{{"fetched_at":{fetched_at},"value":"production"}}"#),
    )
    .expect("write NATS cache");

    fs::write(
        config_dir.join("config.json"),
        r#"{
            "theme": "rainbow",
            "update": { "disable": true },
            "rows": [{ "left": ["nats"] }]
        }"#,
    )
    .expect("write NATS config");

    let output = Command::new(BIN)
        .args(["show", "fish", "-s", "0", "-c", "80"])
        .env("HOME", &home)
        .env("USERPROFILE", &home)
        .env("XDG_CACHE_HOME", &cache)
        .env("LOCALAPPDATA", &cache)
        .env("PATH", &bin_dir)
        .output()
        .expect("run superline");

    assert!(
        output.status.success(),
        "show failed:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let prompt = String::from_utf8_lossy(&output.stdout);
    assert!(
        prompt.contains(&format!("{NATS_ICON}production")),
        "prompt: {prompt}"
    );

    let _ = fs::remove_dir_all(root);
}

#[test]
fn missing_nats_cli_hides_the_widget_without_starting_a_refresh() {
    let root = scratch_dir("missing");
    let home = root.join("home");
    let cache = root.join("cache");
    let bin_dir = root.join("bin");
    let config_dir = home.join(".config/superline");
    fs::create_dir_all(&config_dir).expect("create config directory");
    fs::create_dir_all(&bin_dir).expect("create bin directory");
    fs::write(
        config_dir.join("config.json"),
        r#"{
            "theme": "rainbow",
            "update": { "disable": true },
            "rows": [{ "left": ["nats"] }]
        }"#,
    )
    .expect("write NATS config");

    let output = Command::new(BIN)
        .args(["show", "fish", "-s", "0", "-c", "80"])
        .env("HOME", &home)
        .env("USERPROFILE", &home)
        .env("XDG_CACHE_HOME", &cache)
        .env("LOCALAPPDATA", &cache)
        .env("PATH", &bin_dir)
        .output()
        .expect("run superline");

    assert!(output.status.success());
    let prompt = String::from_utf8_lossy(&output.stdout);
    assert!(!prompt.contains(NATS_ICON), "prompt: {prompt}");
    assert!(!cache.join("superline").exists());

    let _ = fs::remove_dir_all(root);
}

#[cfg(unix)]
#[test]
fn lookup_runs_nats_context_info_as_json() {
    use std::os::unix::fs::PermissionsExt;

    let root = scratch_dir("lookup");
    let binary = root.join("nats");
    fs::write(
        &binary,
        b"#!/bin/sh\n[ \"$1 $2 $3\" = \"context info --json\" ] || exit 1\nprintf '%s' '{\"name\":\"staging\"}'\n",
    )
    .expect("write nats fixture");
    fs::set_permissions(&binary, fs::Permissions::from_mode(0o755))
        .expect("make fixture executable");

    let lookup = NatsLookup { binary };
    assert_eq!(lookup.fetch().as_deref(), Some("staging"));

    let _ = fs::remove_dir_all(root);
}
