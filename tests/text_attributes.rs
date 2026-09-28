//! Theme text attributes (`bold`, `italic`, `underline`) in each shell's
//! prompt: wrapped like the colour escapes, and never left on past the text
//! they style.

use std::fs;
use std::process::Command;

const BIN: &str = env!("CARGO_BIN_EXE_superline");

const CONFIG: &str = r#"{
    "theme": "theme.json",
    "update": { "disable": true },
    "rows": [
        {
            "left": [{ "text": "1" }, "shell", { "text": "2" }],
            "right": ["shell", { "text": "3" }]
        },
        { "left": ["shell", { "text": "4" }], "right": [{ "text": "5" }, "shell"] }
    ]
}"#;

const THEME: &str = r#"{
    "defaults": { "fg": 15, "bg": 236 },
    "modules": { "shell": { "fg": 1, "bg": 31, "bold": true, "underline": true } }
}"#;

/// Runs `superline <command> <shell>` on [`CONFIG`], returning its stdout.
fn render(command: &str, shell: &str) -> String {
    let dir = std::env::temp_dir().join(format!(
        "superline-attrs-{}-{command}-{shell}",
        std::process::id()
    ));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).expect("create scratch dir");
    fs::write(dir.join("config.json"), CONFIG).expect("write config");
    fs::write(dir.join("theme.json"), THEME).expect("write theme");

    let output = Command::new(BIN)
        .args([command, shell, "-s", "0", "-c", "60", "--config"])
        .arg(dir.join("config.json"))
        .current_dir(&dir)
        .env("HOME", &dir)
        .env("USERPROFILE", &dir)
        .env("XDG_CACHE_HOME", dir.join("cache"))
        .env("LOCALAPPDATA", dir.join("cache"))
        .output()
        .expect("run superline");
    let _ = fs::remove_dir_all(&dir);
    assert!(
        output.status.success(),
        "`{command} {shell}` failed:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout).into_owned()
}

/// The prompt with bash's and zsh's non-printing markers removed, leaving the
/// escapes a terminal sees.
fn bare(shell: &str, prompt: &str) -> String {
    match shell {
        "bash" => prompt
            .replace("\\[", "")
            .replace("\\]", "")
            .replace("\\e", "\x1b"),
        "zsh" => prompt.replace("%{", "").replace("%}", ""),
        _ => prompt.to_string(),
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
struct Attrs {
    bold: bool,
    italic: bool,
    underline: bool,
}

/// Each printed character with the attributes a terminal would draw it in,
/// and the attributes still on at the end.
fn drawn(prompt: &str) -> (Vec<(char, Attrs)>, Attrs) {
    let mut attrs = Attrs::default();
    let mut drawn = Vec::new();
    let mut chars = prompt.chars();
    while let Some(c) = chars.next() {
        if c != '\x1b' {
            if !c.is_control() {
                drawn.push((c, attrs));
            }
            continue;
        }
        if chars.next() != Some('[') {
            continue;
        }
        let params: String = chars.by_ref().take_while(|c| *c != 'm').collect();
        let mut codes = params.split(';');
        while let Some(code) = codes.next() {
            match code {
                "" | "0" => attrs = Attrs::default(),
                "1" => attrs.bold = true,
                "3" => attrs.italic = true,
                "4" => attrs.underline = true,
                "22" => attrs.bold = false,
                "23" => attrs.italic = false,
                "24" => attrs.underline = false,
                "38" | "48" => {
                    codes.next();
                    codes.next();
                }
                _ => {}
            }
        }
    }
    (drawn, attrs)
}

fn check(command: &str, shell: &str, name: &str) {
    let prompt = render(command, shell);
    let (drawn, end) = drawn(&bare(shell, &prompt));
    let styled = Attrs {
        bold: true,
        italic: false,
        underline: true,
    };
    assert!(
        drawn.iter().any(|(c, _)| name.contains(*c)),
        "no {name} segment in {prompt:?}"
    );
    for (c, attrs) in drawn {
        if name.contains(c) {
            assert_eq!(attrs, styled, "{c:?} in {prompt:?}");
        } else if c != ' ' {
            // Separators and the other segments.
            assert_eq!(attrs, Attrs::default(), "{c:?} in {prompt:?}");
        }
    }
    assert_eq!(
        end,
        Attrs::default(),
        "attributes left on for the command line: {prompt:?}"
    );
}

#[test]
fn bash_wraps_attribute_escapes_and_closes_them() {
    let prompt = render("show", "bash");
    assert!(prompt.contains(r"\[\e[38;5;1m\]\[\e[1;4m\]"), "{prompt:?}");
    assert!(prompt.contains(r"\[\e[22;24m\]"), "{prompt:?}");
    check("show", "bash", "bash");
    check("show-right", "bash", "bash");
}

#[test]
fn zsh_wraps_attribute_escapes_and_closes_them() {
    let prompt = render("show", "zsh");
    assert!(
        prompt.contains("%{\x1b[38;5;1m%}%{\x1b[1;4m%}"),
        "{prompt:?}"
    );
    assert!(prompt.contains("%{\x1b[22;24m%}"), "{prompt:?}");
    check("show", "zsh", "zsh");
    check("show-right", "zsh", "zsh");
}

#[test]
fn bare_shells_emit_attribute_escapes_and_close_them() {
    let prompt = render("show", "fish");
    assert!(prompt.contains("\x1b[38;5;1m\x1b[1;4m"), "{prompt:?}");
    assert!(prompt.contains("\x1b[22;24m"), "{prompt:?}");
    check("show", "fish", "fish");
    check("show-right", "fish", "fish");
}

#[test]
fn a_non_boolean_attribute_is_a_theme_error() {
    let dir = std::env::temp_dir().join(format!("superline-attrs-{}-invalid", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).expect("create scratch dir");
    fs::write(
        dir.join("config.json"),
        r#"{ "theme": "theme.json", "update": { "disable": true }, "rows": [{ "left": ["shell"] }] }"#,
    )
    .expect("write config");
    fs::write(
        dir.join("theme.json"),
        r#"{ "defaults": { "fg": 15, "bg": 0 }, "modules": { "shell": { "bold": "yes" } } }"#,
    )
    .expect("write theme");

    let output = Command::new(BIN)
        .args(["preview", "-s", "0", "-c", "60", "fish", "--config"])
        .arg(dir.join("config.json"))
        .env("HOME", &dir)
        .env("USERPROFILE", &dir)
        .env("XDG_CACHE_HOME", dir.join("cache"))
        .env("LOCALAPPDATA", dir.join("cache"))
        .output()
        .expect("run superline");
    let _ = fs::remove_dir_all(&dir);
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("expected true or false at modules.shell.bold"),
        "{stderr}"
    );
}
