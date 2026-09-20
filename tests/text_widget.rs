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
    render_shell(label, "fish", text)
}

fn render_shell(label: &str, shell: &str, text: &str) -> String {
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
        .args(["show", shell, "-s", "0", "-c", "80"])
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
    let prompt = render(
        "punctuation",
        r#""café 🌈 $(printf INJECT) `printf INJECT` 100% \\ [brackets]""#,
    );
    assert!(
        prompt.contains(r#"café 🌈 $(printf INJECT) `printf INJECT` 100% \ [brackets]"#),
        "{prompt}"
    );
}

#[test]
fn makes_terminal_controls_visible_without_emitting_them() {
    let prompt = render(
        "controls",
        r#""before\u001b[31m\r\nnext\u0007bell\u0000nul""#,
    );
    assert!(
        prompt.contains(r#"before\u{1b}[31m\r\nnext\u{7}bell\u{0}nul"#),
        "{prompt}"
    );
    assert!(
        !prompt.contains("\x1b[31m"),
        "raw text escape was emitted: {prompt}"
    );
    assert!(!prompt.contains('\r'));
    assert!(!prompt.contains('\x07'));
    assert!(!prompt.contains('\0'));
    assert_eq!(
        prompt.matches('\n').count(),
        1,
        "text introduced a new line: {prompt}"
    );
}

#[test]
fn zsh_init_does_not_reexpand_prompt_text() {
    let output = Command::new(BIN)
        .args(["init", "zsh"])
        .output()
        .expect("run zsh initializer");
    assert!(output.status.success(), "init zsh failed");

    let init = String::from_utf8_lossy(&output.stdout);
    assert!(
        init.contains("__pl_prompt=\"$(superline show")
            && init.contains("__pl_right_prompt=\"$(superline show-right")
    );
    assert!(init.contains("PS1='$__pl_prompt'") && init.contains("RPS1='$__pl_right_prompt'"));
}

#[test]
fn zsh_promptsubst_keeps_text_commands_literal() {
    if Command::new("zsh")
        .arg("--version")
        .output()
        .map(|output| !output.status.success())
        .unwrap_or(true)
    {
        eprintln!("skipping zsh_promptsubst_keeps_text_commands_literal: zsh not on PATH");
        return;
    }

    let prompt = render_shell(
        "zsh-promptsubst",
        "zsh",
        r#""literal $(printf ZSH_EXECUTED) `printf ZSH_EXECUTED` 100% \\ [brackets]""#,
    );
    let prompt = prompt.trim_end_matches('\n');
    let output = Command::new("zsh")
        .args([
            "-f",
            "-c",
            "setopt promptsubst; __pl_prompt=\"$PL\"; PS1='$__pl_prompt'; print -P \"$PS1\"",
        ])
        .env("PL", prompt)
        .output()
        .expect("run zsh prompt expansion");
    assert!(output.status.success(), "zsh prompt expansion failed");

    let rendered = String::from_utf8_lossy(&output.stdout);
    assert!(rendered.contains("$(printf ZSH_EXECUTED)"));
    assert!(rendered.contains("`printf ZSH_EXECUTED`"));
    assert_eq!(
        rendered.matches("ZSH_EXECUTED").count(),
        2,
        "zsh re-expanded Text content: {rendered}"
    );
}
