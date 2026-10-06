//! A theme colour of `"none"` in each shell's prompt: segments on the
//! terminal's own background (`49`), and the separators that meet them.

use std::fs;
use std::process::Command;

const BIN: &str = env!("CARGO_BIN_EXE_superline");

/// Clear text segments around a coloured `shell` segment, covering clear →
/// coloured, coloured → clear, clear → clear and a clear last segment on both
/// sides.
const CONFIG: &str = r#"{
    "theme": "theme.json",
    "update": { "disable": true },
    "rows": [
        {
            "left": [{ "text": "1" }, "shell", { "text": "2" }, { "text": "3" }],
            "right": [{ "text": "4" }, "shell", { "text": "5" }, { "text": "6" }]
        }
    ]
}"#;

const THEME: &str = r#"{
    "defaults": { "fg": "green", "bg": "none" },
    "modules": { "shell": { "fg": 15, "bg": 31 } }
}"#;

const GREEN: Option<u8> = Some(2);
const BLUE: Option<u8> = Some(31);

/// Runs `superline <command> <shell>` on [`CONFIG`], returning its stdout.
fn render(command: &str, shell: &str) -> String {
    let dir = std::env::temp_dir().join(format!(
        "superline-clear-{}-{command}-{shell}",
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

/// A printed character and its foreground and background, `None` for the
/// terminal's own colour.
type Cell = (char, Option<u8>, Option<u8>);

/// Each printed character with the colours a terminal would draw it in, and
/// the background still on at the end.
fn drawn(prompt: &str) -> (Vec<Cell>, Option<u8>) {
    let (mut fg, mut bg) = (None, None);
    let mut cells = Vec::new();
    let mut chars = prompt.chars();
    while let Some(c) = chars.next() {
        if c != '\x1b' {
            if !c.is_control() {
                cells.push((c, fg, bg));
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
                "" | "0" => (fg, bg) = (None, None),
                "39" => fg = None,
                "49" => bg = None,
                "38" | "48" => {
                    assert_eq!(codes.next(), Some("5"), "{prompt:?}");
                    let color = codes.next().and_then(|n| n.parse().ok());
                    if code == "38" {
                        fg = color;
                    } else {
                        bg = color;
                    }
                }
                _ => {}
            }
        }
    }
    (cells, bg)
}

/// The powerline glyphs in `cells`, with their colours.
fn glyphs(cells: &[Cell]) -> Vec<Cell> {
    cells
        .iter()
        .copied()
        .filter(|(c, ..)| ('\u{e0b0}'..='\u{e0b7}').contains(c))
        .collect()
}

/// The escape that selects the terminal's own background, as `shell` wraps it.
fn default_bg(shell: &str) -> &'static str {
    match shell {
        "bash" => r"\[\e[49m\]",
        "zsh" => "%{\x1b[49m%}",
        _ => "\x1b[49m",
    }
}

fn check(shell: &str) {
    for (command, expected) in [
        (
            "show",
            [
                // 1 → shell: the coloured segment opens with a cap.
                ('\u{e0b2}', BLUE, None),
                // shell → 2: the arrow in the coloured background.
                ('\u{e0b0}', BLUE, None),
                // 2 → 3: the outline arrow in 2's text colour.
                ('\u{e0b1}', GREEN, None),
            ],
        ),
        (
            "show-right",
            [
                // 4 → shell: the usual opening arrow.
                ('\u{e0b2}', BLUE, None),
                // shell → 5: the coloured segment closes with a cap.
                ('\u{e0b0}', BLUE, None),
                // 5 → 6: the outline arrow in 6's text colour.
                ('\u{e0b3}', GREEN, None),
            ],
        ),
    ] {
        let prompt = render(command, shell);
        assert!(prompt.contains(default_bg(shell)), "{prompt:?}");
        let (cells, end_bg) = drawn(&bare(shell, &prompt));
        assert_eq!(glyphs(&cells), expected, "{command} {shell}: {prompt:?}");
        for (c, fg, bg) in &cells {
            if "123456".contains(*c) {
                assert_eq!((*fg, *bg), (GREEN, None), "{c:?} in {prompt:?}");
            } else if shell.contains(*c) {
                assert_eq!((*fg, *bg), (Some(15), BLUE), "{c:?} in {prompt:?}");
            } else if *c == ' ' {
                // Only the shell segment has a background; spaces next to
                // the clear segments sit on the terminal's.
                assert_eq!(*bg, None, "space in {prompt:?}");
            }
        }
        assert_eq!(end_bg, None, "background left on: {prompt:?}");
    }
}

#[test]
fn bash_draws_clear_segments_on_the_default_background() {
    check("bash");
}

#[test]
fn zsh_draws_clear_segments_on_the_default_background() {
    check("zsh");
}

#[test]
fn fish_draws_clear_segments_on_the_default_background() {
    check("fish");
}

#[test]
fn pwsh_draws_clear_segments_on_the_default_background() {
    check("pwsh");
}

#[test]
fn nushell_draws_clear_segments_on_the_default_background() {
    check("nu");
}

#[test]
fn clear_text_uses_the_default_foreground() {
    let dir = std::env::temp_dir().join(format!("superline-clear-{}-fg", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).expect("create scratch dir");
    fs::write(
        dir.join("config.json"),
        r#"{ "theme": "theme.json", "update": { "disable": true }, "rows": [{ "left": ["shell"] }] }"#,
    )
    .expect("write config");
    fs::write(
        dir.join("theme.json"),
        r#"{ "defaults": { "fg": "none", "bg": 31 }, "modules": {} }"#,
    )
    .expect("write theme");
    let output = Command::new(BIN)
        .args(["show", "fish", "-s", "0", "-c", "60", "--config"])
        .arg(dir.join("config.json"))
        .env("HOME", &dir)
        .env("USERPROFILE", &dir)
        .env("XDG_CACHE_HOME", dir.join("cache"))
        .env("LOCALAPPDATA", dir.join("cache"))
        .output()
        .expect("run superline");
    let _ = fs::remove_dir_all(&dir);
    assert!(output.status.success());
    let prompt = String::from_utf8_lossy(&output.stdout);
    assert!(prompt.contains("\x1b[48;5;31m\x1b[39m"), "{prompt:?}");
    let (cells, _) = drawn(&prompt);
    let f = cells.iter().find(|cell| cell.0 == 'f').unwrap();
    assert_eq!(*f, ('f', None, BLUE));
}
