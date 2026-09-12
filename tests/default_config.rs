//! End-to-end check that the compiled binary can generate and parse its
//! default configuration.
//!
//! On first run with no existing config, `superline show` writes
//! `Config::default()` to `$HOME/.config/superline/config.json` and then
//! immediately reads it back. If that round-trip ever broke, a fresh install
//! would fail to render a prompt. This test drives the real binary against a
//! throwaway `$HOME` to make sure that path stays healthy.

use std::fs;
use std::path::PathBuf;
use std::process::Command;

/// Path to the binary compiled by Cargo for this integration test run.
const BIN: &str = env!("CARGO_BIN_EXE_superline");

fn scratch_home() -> PathBuf {
    let dir = std::env::temp_dir().join(format!("superline-it-{}", std::process::id()));
    // Start from a clean slate so we exercise the "create default config" path.
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).expect("create scratch home");
    dir
}

#[test]
fn default_config_parses_with_compiled_binary() {
    let home = scratch_home();

    let output = Command::new(BIN)
        .args(["show", "fish", "-s", "0", "-c", "80"])
        // Point the binary's home lookup at the scratch dir on both Unix ($HOME)
        // and Windows (%USERPROFILE%).
        .env("HOME", &home)
        .env("USERPROFILE", &home)
        .env("SUPERLINE_DISABLE_SERVER", "1")
        .output()
        .expect("failed to run the powerline binary");

    let stderr = String::from_utf8_lossy(&output.stderr);

    // The default config is created on this path before anything is rendered.
    let config_path = home.join(".config/superline/config.json");
    assert!(
        config_path.is_file(),
        "binary did not create the default config at {}\nstderr:\n{stderr}",
        config_path.display(),
    );

    // `load_config` reports a parse failure with this exact message. Its absence
    // means the freshly written default config deserialized cleanly.
    assert!(
        !stderr.contains("config file could not be parsed"),
        "default config failed to parse via the binary\nstderr:\n{stderr}",
    );

    let _ = fs::remove_dir_all(&home);
}
