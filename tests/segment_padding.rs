//! Segment padding: a widget's `padding` option wins over the theme's
//! per-module `padding`, which wins over the widget's own spacing.

use std::fs;
use std::path::PathBuf;
use std::process::Command;

const BIN: &str = env!("CARGO_BIN_EXE_superline");

const COLUMNS: usize = 100;

/// `nvm` is the key the `node` module used to be themed under.
const THEME: &str = r#"{
    "defaults": { "fg": 15, "bg": 0 },
    "modules": {
        "text": { "padding": 3 },
        "shell": { "padding": 2 },
        "nvm": { "padding": 0 }
    }
}"#;

fn scratch_dir(label: &str) -> PathBuf {
    let dir =
        std::env::temp_dir().join(format!("superline-padding-{}-{label}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).expect("create scratch dir");
    dir
}

/// The visible text of the preview of a one-row config.
fn preview(label: &str, left: &str, right: &str) -> String {
    let home = scratch_dir(label);
    let project = home.join("project");
    fs::create_dir_all(&project).expect("create project dir");
    fs::write(project.join(".nvmrc"), "20.1.0\n").expect("write .nvmrc");
    fs::write(home.join("theme.json"), THEME).expect("write theme");
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
    assert!(
        output.status.success(),
        "preview failed:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
    visible(&String::from_utf8(output.stdout).expect("utf-8 prompt"))
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
fn config_padding_wins_over_the_theme_which_wins_over_the_default() {
    let widgets = r#"
        { "text": "themed" },
        { "shell": { "padding": 1 } },
        "cmd",
        { "cmd": { "padding": 2 } },
        "node"
    "#;
    let line = preview("precedence", widgets, widgets);

    // text: theme 3. shell: config 1 over theme 2. cmd: its own 0, then
    // config 2. node: theme 0, read from the old `nvm` key.
    let left = "   themed   \u{e0b0} fish \u{e0b0}$\u{e0b0}  $  \u{e0b0}\u{ed0d} 20.1.0\u{e0b0}";
    let right = "\u{e0b2}   themed   \u{e0b2} fish \u{e0b2}$\u{e0b2}  $  \u{e0b2}\u{ed0d} 20.1.0";
    assert!(line.starts_with(left), "{line:?}");
    assert!(line.ends_with(right), "{line:?}");
    // The right side is pushed flush against the last column but one, so the
    // padded segments were counted at their drawn width.
    assert_eq!(line.chars().count(), COLUMNS - 1, "{line:?}");
}
