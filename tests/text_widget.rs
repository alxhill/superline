//! End-to-end checks for the literal Text widget.

use std::fs;
use std::path::PathBuf;
use std::process::Command;

const BIN: &str = env!("CARGO_BIN_EXE_superline");

fn scratch_dir(label: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("superline-text-{}-{label}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).expect("create scratch dir");
    dir
}

fn render(label: &str, text: &str) -> String {
    let root = scratch_dir(label);
    let home = root.join("home");
    let config_dir = home.join(".config/superline");
    fs::create_dir_all(&config_dir).expect("create config dir");
    fs::write(
        config_dir.join("config.json"),
        format!(
            r#"{{
                "theme": "rainbow",
                "update": {{ "disable": true }},
                "rows": [{{ "left": [{{ "text": {text} }}] }}]
            }}"#
        ),
    )
    .expect("write text config");

    let output = Command::new(BIN)
        .args(["show", "fish", "-s", "0", "-c", "80"])
        .env("HOME", &home)
        .env("USERPROFILE", &home)
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
fn renders_unicode_and_shell_punctuation_literally() {
    let prompt = render("punctuation", r#""café 🌈 $HOME 100% \\ [brackets]""#);
    assert!(
        prompt.contains(r#"café 🌈 $HOME 100% \ [brackets]"#),
        "{prompt}"
    );
}

#[test]
fn makes_terminal_controls_visible_without_emitting_them() {
    let prompt = render("controls", r#""before\u001b[31m\nnext""#);
    assert!(prompt.contains(r#"before\u{1b}[31m\nnext"#), "{prompt}");
    assert!(
        !prompt.contains("\x1b[31m"),
        "raw text escape was emitted: {prompt}"
    );
}
