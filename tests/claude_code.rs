//! End-to-end checks for `superline claude-code` and
//! `superline install claude-code`.

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};

const BIN: &str = env!("CARGO_BIN_EXE_superline");

struct Scratch {
    root: PathBuf,
}

impl Scratch {
    fn new(label: &str) -> Scratch {
        let root = std::env::temp_dir().join(format!(
            "superline-claude-code-{}-{label}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(root.join("home")).expect("create scratch home");
        Scratch { root }
    }

    fn home(&self) -> PathBuf {
        self.root.join("home")
    }

    fn layout(&self, rows: &str) -> PathBuf {
        let path = self.root.join("claude-code.json");
        fs::write(
            &path,
            format!(r#"{{ "theme": "rainbow", "rows": {rows} }}"#),
        )
        .expect("write layout");
        path
    }

    fn command(&self) -> Command {
        let mut command = Command::new(BIN);
        command
            .current_dir(&self.root)
            .env("HOME", self.home())
            .env("USERPROFILE", self.home())
            .env("XDG_CACHE_HOME", self.root.join("cache"))
            .env("CLAUDE_CONFIG_DIR", self.root.join("claude"))
            .env("COLUMNS", "120")
            .env_remove("PWD");
        command
    }

    /// Runs `superline claude-code` with `stdin` as the session data.
    fn render(&self, args: &[&str], stdin: &str) -> String {
        let mut child = self
            .command()
            .arg("claude-code")
            .args(args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .expect("run superline claude-code");
        child
            .stdin
            .take()
            .unwrap()
            .write_all(stdin.as_bytes())
            .unwrap();
        let output = child.wait_with_output().unwrap();
        assert!(
            output.status.success(),
            "claude-code failed:\n{}",
            String::from_utf8_lossy(&output.stderr)
        );
        String::from_utf8(output.stdout).unwrap()
    }

    fn install(&self, args: &[&str]) -> Output {
        self.command()
            .args(["install", "claude-code"])
            .args(args)
            .output()
            .expect("run superline install claude-code")
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

/// The printed text without colour escapes or hyperlink wrappers.
fn visible(text: &str) -> String {
    let mut out = String::new();
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        if c != '\x1b' {
            out.push(c);
            continue;
        }
        match chars.next() {
            Some('[') => {
                for c in chars.by_ref() {
                    if c.is_ascii_alphabetic() {
                        break;
                    }
                }
            }
            Some(']') => {
                while let Some(c) = chars.next() {
                    if c == '\x1b' && chars.peek() == Some(&'\\') {
                        chars.next();
                        break;
                    }
                }
            }
            _ => {}
        }
    }
    out
}

const SESSION: &str = r#"{
    "session_name": "statusline work",
    "model": { "id": "claude-opus-5-5", "display_name": "Opus 5.5" },
    "cost": {
        "total_cost_usd": 3.4721,
        "total_duration_ms": 3900000,
        "total_api_duration_ms": 45000,
        "total_lines_added": 412,
        "total_lines_removed": 87
    },
    "context_window": {
        "total_input_tokens": 92400,
        "context_window_size": 200000,
        "used_percentage": 46.2
    },
    "prompt_cache": { "warm": true, "caching_observed": true, "hit_ratio": 0.91 },
    "fast_mode": true,
    "effort": { "level": "high" },
    "rate_limits": {
        "five_hour": { "used_percentage": 23.5, "resets_at": 1738425600 },
        "seven_day": { "used_percentage": 41.2, "resets_at": 1738857600 }
    },
    "vim": { "mode": "INSERT" },
    "agent": { "name": "reviewer" },
    "a_field_from_a_newer_claude_code": { "nested": [1, 2, 3] }
}"#;

#[test]
fn draws_every_claude_code_widget_from_the_session_data() {
    let scratch = Scratch::new("widgets");
    let layout = scratch.layout(
        r#"[{ "left": [
            { "claude_model": {} },
            { "claude_context": { "tokens": true } },
            "claude_cost",
            "claude_duration",
            { "claude_duration": { "api": true } },
            "claude_lines",
            "claude_cache",
            "claude_vim",
            "claude_agent",
            { "claude_session": { "max_length": 10 } }
        ] }]"#,
    );
    let line = visible(&scratch.render(&["--config", layout.to_str().unwrap()], SESSION));

    for expected in [
        "Opus 5.5 \u{f140b} high",
        "46% 92k/200k",
        "$3.47",
        "1h 5m",
        "45s",
        "+412 -87",
        "91%",
        "INSERT",
        "reviewer",
        "statuslin…",
    ] {
        assert!(line.contains(expected), "missing {expected:?} in {line:?}");
    }
}

#[test]
fn widget_options_turn_parts_off() {
    let scratch = Scratch::new("options");
    let layout = scratch.layout(
        r#"[{ "left": [
            { "claude_model": { "effort": false, "fast_mode": false } },
            { "claude_context": { "display": "block" } },
            { "claude_context": { "display": "bar", "width": 10 } }
        ] }]"#,
    );
    let line = visible(&scratch.render(&["--config", layout.to_str().unwrap()], SESSION));
    assert!(line.contains("Opus 5.5 "), "{line:?}");
    assert!(!line.contains("high"), "{line:?}");
    assert!(!line.contains('\u{f140b}'), "{line:?}");
    assert!(line.contains(" ██▒░░ "), "{line:?}");
    assert!(line.contains(" ▄▄▄▄▄▁▁▁▁▁ "), "{line:?}");
}

#[test]
fn claude_code_widgets_are_empty_without_session_data() {
    let scratch = Scratch::new("empty");
    let layout = scratch.layout(
        r#"[{ "left": [{ "text": "here" }, "claude_model", "claude_cost", "claude_vim"] }]"#,
    );
    let line = visible(&scratch.render(&["--config", layout.to_str().unwrap()], ""));
    assert_eq!(line.trim_end(), " here \u{e0b0}");
}

#[test]
fn rows_with_nothing_to_show_are_left_out() {
    let scratch = Scratch::new("rows");
    let layout = scratch.layout(
        r#"[
            { "left": ["claude_model"] },
            { "left": [{ "separator": "round" }, { "padding": 1 }, "claude_agent"] },
            { "left": ["claude_cost"] }
        ]"#,
    );
    let output = scratch.render(
        &["--config", layout.to_str().unwrap()],
        r#"{ "model": { "display_name": "Sonnet" }, "cost": { "total_cost_usd": 0.5 } }"#,
    );
    let lines: Vec<String> = output.lines().map(visible).collect();
    assert_eq!(lines.len(), 2, "{lines:?}");
    assert!(lines[0].contains("Sonnet"));
    assert!(lines[1].contains("$0.50"));
}

#[test]
fn directory_widgets_look_at_the_session_directory() {
    let scratch = Scratch::new("cwd");
    let project = scratch.root.join("session-project");
    fs::create_dir_all(&project).unwrap();
    let layout =
        scratch.layout(r#"[{ "left": [{ "cwd": { "max_length": 200, "wanted_seg_num": 20 } }] }]"#);
    let session = format!(
        r#"{{ "cwd": "/elsewhere", "workspace": {{ "current_dir": {} }} }}"#,
        serde_json::to_string(project.to_str().unwrap()).unwrap()
    );
    let line = visible(&scratch.render(&["--config", layout.to_str().unwrap()], &session));
    assert!(line.contains("session-project"), "{line:?}");
}

#[test]
fn ai_usage_shows_the_rate_limits_claude_code_reports() {
    let scratch = Scratch::new("usage");
    let layout = scratch.layout(r#"[{ "left": [{ "ai_usage": { "provider": "claude" } }] }]"#);
    let line = visible(&scratch.render(&["--config", layout.to_str().unwrap()], SESSION));
    assert!(line.contains("5h 24% 7d 41%"), "{line:?}");
    // Nothing was looked up in the background.
    let cached: Vec<_> = fs::read_dir(scratch.root.join("cache/superline"))
        .map(|entries| {
            entries
                .filter_map(Result::ok)
                .map(|entry| entry.file_name().to_string_lossy().into_owned())
                .filter(|name| name.contains("usage"))
                .collect()
        })
        .unwrap_or_default();
    assert!(cached.is_empty(), "{cached:?}");
}

#[test]
fn ai_usage_leaves_out_the_hover_text_claude_code_would_drop() {
    let scratch = Scratch::new("hover");
    let layout = scratch.layout(
        r#"[{ "left": [{ "ai_usage": { "provider": "claude", "display": "sparkline" } }] }]"#,
    );
    let output = scratch.render(&["--config", layout.to_str().unwrap()], SESSION);
    assert!(visible(&output).contains("5h"), "{output:?}");
    assert!(!output.contains("1337"), "{output:?}");
}

#[test]
fn pr_falls_back_to_the_pull_request_claude_code_reports() {
    let scratch = Scratch::new("pr");
    let layout = scratch.layout(r#"[{ "left": ["pr"] }]"#);
    let output = scratch.render(
        &["--config", layout.to_str().unwrap()],
        r#"{ "pr": { "number": 1234, "url": "https://example.com/pull/1234", "review_state": "draft" } }"#,
    );
    assert!(visible(&output).contains("#1234"), "{output:?}");
    assert!(
        output.contains("https://example.com/pull/1234"),
        "{output:?}"
    );
}

#[test]
fn the_right_side_is_aligned_to_columns_less_the_margin() {
    let scratch = Scratch::new("columns");
    let layout = scratch.layout(
        r#"[{ "left": [{ "separator": "none" }, { "text": "L" }],
              "right": [{ "separator": "none" }, { "text": "R" }] }]"#,
    );
    let layout = layout.to_str().unwrap();
    let width = |args: &[&str]| {
        let mut all = vec!["--config", layout];
        all.extend_from_slice(args);
        visible(&scratch.render(&all, ""))
            .trim_end_matches('\n')
            .chars()
            .count()
    };
    // One column is always left spare, as in the shell prompt.
    assert_eq!(width(&["--columns", "30"]), 29);
    assert_eq!(width(&[]), 120 - 4 - 1);
    assert_eq!(width(&["--margin", "10"]), 120 - 10 - 1);
}

#[test]
fn a_missing_layout_is_created_silently_with_the_default() {
    let scratch = Scratch::new("default");
    let output = scratch.render(&[], SESSION);
    let line = visible(&output);
    assert!(line.contains("Opus 5.5"), "{line:?}");
    assert!(!line.contains("config"), "{line:?}");
    assert_eq!(output.lines().count(), 1, "{output:?}");

    let layout = scratch.home().join(".config/superline/claude-code.json");
    let written: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(&layout).unwrap()).unwrap();
    assert_eq!(written["theme"], "rainbow");
    assert!(scratch
        .home()
        .join(".config/superline/rainbow.json")
        .exists());
}

#[test]
fn problems_show_in_the_status_line_instead_of_blanking_it() {
    let scratch = Scratch::new("errors");
    let layout = scratch.layout(r#"[{ "left": ["claude_cost"] }]"#);
    let line = visible(&scratch.render(&["--config", layout.to_str().unwrap()], "not json"));
    assert!(line.contains("status data not parsed"), "{line:?}");

    fs::write(&layout, "{ broken").unwrap();
    let line = visible(&scratch.render(&["--config", layout.to_str().unwrap()], SESSION));
    assert!(line.contains("config file not parsed"), "{line:?}");
    assert!(
        line.contains("Opus 5.5"),
        "falls back to the default: {line:?}"
    );
}

#[test]
fn the_preview_reports_a_broken_layout_on_stderr() {
    let scratch = Scratch::new("preview");
    let layout = scratch.root.join("broken.json");
    fs::write(&layout, "{ broken").unwrap();
    let output = scratch
        .command()
        .args(["claude-code", "--sample", "--preview", "--config"])
        .arg(&layout)
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("could not be parsed"));
}

#[test]
fn the_sample_fills_the_default_layout() {
    let scratch = Scratch::new("sample");
    let output = scratch
        .command()
        .args(["claude-code", "--sample"])
        .stdin(Stdio::null())
        .output()
        .unwrap();
    let line = visible(&String::from_utf8(output.stdout).unwrap());
    for expected in ["Opus 5.5", "46%", "$3.47", "+412 -87", "5h 24%"] {
        assert!(line.contains(expected), "missing {expected:?} in {line:?}");
    }
}

fn settings(scratch: &Scratch) -> serde_json::Value {
    let path = scratch.root.join("claude/settings.json");
    serde_json::from_str(&fs::read_to_string(path).unwrap()).unwrap()
}

#[test]
fn install_points_claude_code_at_superline() {
    let scratch = Scratch::new("install");
    let output = scratch.install(&[]);
    assert!(output.status.success(), "{output:?}");
    assert_eq!(
        settings(&scratch)["statusLine"],
        serde_json::json!({ "type": "command", "command": "superline claude-code", "padding": 0 })
    );
    assert!(scratch
        .home()
        .join(".config/superline/claude-code.json")
        .exists());

    let again = scratch.install(&[]);
    assert!(again.status.success());
    assert!(String::from_utf8_lossy(&again.stdout).contains("already uses superline"));
}

#[test]
fn install_keeps_other_settings_and_only_replaces_a_status_line_when_forced() {
    let scratch = Scratch::new("install-force");
    let dir = scratch.root.join("claude");
    fs::create_dir_all(&dir).unwrap();
    fs::write(
        dir.join("settings.json"),
        r#"{ "model": "opus", "statusLine": { "type": "command", "command": "~/line.sh" } }"#,
    )
    .unwrap();

    let refused = scratch.install(&[]);
    assert!(!refused.status.success());
    assert!(String::from_utf8_lossy(&refused.stderr).contains("~/line.sh"));
    assert_eq!(settings(&scratch)["statusLine"]["command"], "~/line.sh");

    let forced = scratch.install(&["--force"]);
    assert!(forced.status.success(), "{forced:?}");
    let settings = settings(&scratch);
    assert_eq!(settings["statusLine"]["command"], "superline claude-code");
    assert_eq!(settings["model"], "opus");
}

#[test]
fn settings_dir_defaults_to_dot_claude_in_home() {
    let scratch = Scratch::new("install-home");
    let output = scratch
        .command()
        .env_remove("CLAUDE_CONFIG_DIR")
        .args(["install", "claude-code"])
        .output()
        .unwrap();
    assert!(output.status.success(), "{output:?}");
    assert!(Path::new(&scratch.home().join(".claude/settings.json")).exists());
}
