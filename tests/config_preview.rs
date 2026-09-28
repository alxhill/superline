//! `superline preview`, which draws the `superline config` editor's preview.

use std::fs;
use std::path::PathBuf;
use std::process::{Command, Output};

const BIN: &str = env!("CARGO_BIN_EXE_superline");

fn preview(label: &str, config: &str) -> Output {
    let dir =
        std::env::temp_dir().join(format!("superline-preview-{}-{label}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).expect("create scratch dir");
    let path: PathBuf = dir.join("config.json");
    fs::write(&path, config).expect("write config");

    let output = Command::new(BIN)
        .args(["preview", "-s", "0", "-c", "80", "fish", "--config"])
        .arg(&path)
        .output()
        .expect("failed to run superline");
    let _ = fs::remove_dir_all(&dir);
    output
}

#[test]
fn every_row_is_drawn_with_its_right_side() {
    let output = preview(
        "rows",
        r#"{
            "theme": "simple",
            "rows": [
                { "left": [{ "text": "top-left" }], "right": [{ "text": "top-right" }] },
                { "left": [{ "text": "last-left" }], "right": [{ "text": "last-right" }] }
            ]
        }"#,
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(output.status.success(), "{output:?}");

    let lines: Vec<&str> = stdout.lines().collect();
    assert_eq!(lines.len(), 2, "{stdout}");
    assert!(lines[0].contains("top-left") && lines[0].contains("top-right"));
    // `show` leaves the last row's right side to the shell; the preview
    // draws it.
    assert!(lines[1].contains("last-left") && lines[1].contains("last-right"));
    assert!(
        !stdout.contains("\\["),
        "preview uses bare escapes: {stdout}"
    );
}

#[test]
fn a_broken_config_is_reported_on_stderr() {
    let output = preview(
        "broken",
        r#"{ "theme": "simple", "rows": [{ "left": [{ "padding": "wide" }] }] }"#,
    );
    assert!(!output.status.success());
    assert!(output.stdout.is_empty());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("could not be parsed"), "{stderr}");
}

#[test]
fn a_missing_theme_is_reported_on_stderr() {
    let output = preview(
        "theme",
        r#"{ "theme": "missing-theme.json", "rows": [{ "left": ["cmd"] }] }"#,
    );
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("missing-theme.json"), "{stderr}");
}
