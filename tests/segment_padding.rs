//! Segment padding: a widget's `padding` option wins over the widget's own
//! spacing. Themes take no padding.

use std::fs;
use std::path::PathBuf;
use std::process::{Command, Output};

const BIN: &str = env!("CARGO_BIN_EXE_superline");

const COLUMNS: usize = 100;

const THEME: &str = r#"{ "defaults": { "fg": 15, "bg": 0 }, "modules": {} }"#;

/// A theme written for v0.23.0, which read `padding` per module, including
/// under `nvm`, the key the `node` module used to be themed under.
const PADDED_THEME: &str = r#"{
    "defaults": { "fg": 15, "bg": 0 },
    "modules": {
        "text": { "padding": "small" },
        "shell": { "padding": "right" },
        "cmd": { "padding": "large" },
        "nvm": { "padding": "left" },
        "git": { "padding": "wide" }
    }
}"#;

const WIDGETS: &str = r#"
    { "text": "themed" },
    { "shell": { "padding": "left" } },
    "shell",
    "cmd",
    { "cmd": { "padding": "large" } },
    { "cmd": { "padding": "right" } },
    "node",
    { "node": { "padding": "small" } }
"#;

fn scratch_dir(label: &str) -> PathBuf {
    let dir =
        std::env::temp_dir().join(format!("superline-padding-{}-{label}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).expect("create scratch dir");
    dir
}

/// Previews a one-row config using `theme`.
fn run(label: &str, theme: &str, left: &str, right: &str) -> Output {
    let home = scratch_dir(label);
    let project = home.join("project");
    fs::create_dir_all(&project).expect("create project dir");
    fs::write(project.join(".nvmrc"), "20.1.0\n").expect("write .nvmrc");
    fs::write(home.join("theme.json"), theme).expect("write theme");
    let config = home.join("config.json");
    fs::write(
        &config,
        format!(
            r#"{{
                "theme": "theme.json",
                "update": {{ "disable": true }},
                "rows": [{{ "left": [{left}], "right": [{right}] }}]
            }}"#
        ),
    )
    .expect("write config");

    let output = Command::new(BIN)
        .args(["preview", "-s", "0", "-c", &COLUMNS.to_string(), "fish"])
        .arg("--config")
        .arg(&config)
        .current_dir(&project)
        .env("HOME", &home)
        .env("USERPROFILE", &home)
        .env("XDG_CACHE_HOME", home.join("cache"))
        .env_remove("nvm_current_version")
        .output()
        .expect("run superline");
    let _ = fs::remove_dir_all(&home);
    output
}

/// The preview of a one-row config using `theme`.
fn preview(label: &str, theme: &str, left: &str, right: &str) -> String {
    let output = run(label, theme, left, right);
    assert!(
        output.status.success(),
        "preview failed:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).expect("utf-8 prompt")
}

/// The prompt with its colour escapes removed.
fn visible(prompt: &str) -> String {
    let mut out = String::new();
    let mut chars = prompt.chars();
    while let Some(c) = chars.next() {
        if c == '\x1b' {
            chars.by_ref().find(|c| c.is_ascii_alphabetic());
        } else {
            out.push(c);
        }
    }
    out.trim_end_matches('\n').to_string()
}

#[test]
fn config_padding_wins_over_the_widget_default() {
    let line = visible(&preview("precedence", THEME, WIDGETS, WIDGETS));

    let segments = [
        // text: its own large.
        " themed ",
        // shell: the config's left, then its own small.
        " fish",
        "fish",
        // cmd: its own small, then the config's large and right.
        "$",
        " $ ",
        "$ ",
        // node: its own large, then the config's small.
        " \u{ed0d} 20.1.0 ",
        "\u{ed0d} 20.1.0",
    ];
    let left: String = segments.iter().map(|s| format!("{s}\u{e0b0}")).collect();
    let right: String = segments.iter().map(|s| format!("\u{e0b2}{s}")).collect();
    assert!(line.starts_with(&left), "{line:?}");
    assert!(line.ends_with(&right), "{line:?}");
    // The right side is pushed flush against the last column but one, so the
    // padded segments were counted at their drawn width.
    assert_eq!(line.chars().count(), COLUMNS - 1, "{line:?}");
}

#[test]
fn a_theme_that_sets_padding_still_loads_and_is_ignored() {
    assert_eq!(
        preview("padded-theme", PADDED_THEME, WIDGETS, WIDGETS),
        preview("plain-theme", THEME, WIDGETS, WIDGETS)
    );
}

#[test]
fn an_invalid_padding_is_reported_with_the_choices() {
    let output = run("bad-config", THEME, r#"{ "git": { "padding": 1 } }"#, "");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(!output.status.success(), "{stderr}");
    assert!(
        stderr.contains("expected one of small, large, left, right"),
        "{stderr}"
    );
}
