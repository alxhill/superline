//! Theme files can replace or hide the icons widgets draw, and the right side
//! of the prompt stays aligned whatever width the replacement has.

use std::fs;
use std::path::PathBuf;
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

use regex::Regex;
use unicode_width::UnicodeWidthStr;

const BIN: &str = env!("CARGO_BIN_EXE_superline");
const COLUMNS: usize = 60;
const CWD: &str = r#"{ "cwd": { "max_length": 40, "wanted_seg_num": 4 } }"#;
const USAGE: &str = r#"{ "ai_usage": { "provider": "claude", "weekly": false } }"#;

struct Scratch {
    root: PathBuf,
}

impl Scratch {
    fn new(label: &str) -> Self {
        let root =
            std::env::temp_dir().join(format!("superline-icons-{}-{label}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        let cache = root.join("cache/superline");
        fs::create_dir_all(&cache).expect("create cache dir");
        fs::create_dir_all(root.join("home/project")).expect("create project dir");

        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("current time");
        // Fresh refresh markers keep the usage widget from launching the
        // provider CLIs, and a fresh reading gives it something to draw.
        for provider in ["claude", "codex"] {
            fs::write(
                cache.join(format!("usage-{provider}.refresh")),
                now.as_millis().to_string(),
            )
            .expect("write refresh marker");
        }
        fs::write(
            cache.join("usage-claude.json"),
            format!(
                r#"{{"fetched_at":{},"value":{{"session":12.4,"weekly":67.8}}}}"#,
                now.as_secs()
            ),
        )
        .expect("write usage cache");

        Scratch { root }
    }

    /// Draws every row of `rows` through `superline preview`, themed by
    /// `modules`, and returns each row's visible text.
    fn render(&self, modules: &str, rows: &str) -> Vec<String> {
        let home = self.root.join("home");
        let project = home.join("project");
        fs::write(
            self.root.join("theme.json"),
            format!(r#"{{"defaults":{{"fg":15,"bg":0}},"modules":{modules}}}"#),
        )
        .expect("write theme");
        let config = self.root.join("config.json");
        fs::write(
            &config,
            format!(r#"{{"theme":"theme.json","update":{{"disable":true}},"rows":{rows}}}"#),
        )
        .expect("write config");

        let output = Command::new(BIN)
            .args(["preview", "-s", "0", "-c", &COLUMNS.to_string()])
            .args(["--jobs", "2", "fish", "--config"])
            .arg(&config)
            .current_dir(&project)
            .env("PWD", &project)
            .env("HOME", &home)
            .env("USERPROFILE", &home)
            .env("XDG_CACHE_HOME", self.root.join("cache"))
            .env("LOCALAPPDATA", self.root.join("cache"))
            .output()
            .expect("run superline");
        assert!(
            output.status.success(),
            "preview failed:\n{}",
            String::from_utf8_lossy(&output.stderr)
        );

        let escapes = Regex::new(r"\x1b\[[0-9;]*m|\x1b\]8;;[^\x1b]*\x1b\\").unwrap();
        String::from_utf8_lossy(&output.stdout)
            .lines()
            .map(|line| escapes.replace_all(line, "").into_owned())
            .collect()
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

#[test]
fn icons_can_be_replaced_or_hidden() {
    let scratch = Scratch::new("replace");
    let rows = scratch.render(
        r#"{
            "cwd": { "home_icon": "" },
            "jobs": { "icon": "" },
            "ai_usage": { "claude_icon": "AI" },
            "os": { "symbol": "" }
        }"#,
        &format!(r#"[{{ "left": [{CWD}, "jobs", {USAGE}] }}, {{ "left": ["os"] }}]"#),
    );

    let prompt = &rows[0];
    assert!(prompt.contains(" project"), "{prompt:?}");
    assert!(
        !prompt.contains('~'),
        "the home icon should be hidden: {prompt:?}"
    );
    assert!(prompt.contains(" 2 "), "{prompt:?}");
    assert!(!prompt.contains("\u{f085}"), "{prompt:?}");
    assert!(prompt.contains(" AI 5h 12% "), "{prompt:?}");
    assert_eq!(rows[1], "", "an empty OS icon hides the widget");
}

#[test]
fn the_default_icons_are_drawn_when_the_theme_sets_none() {
    let scratch = Scratch::new("defaults");
    let rows = scratch.render(
        "{}",
        &format!(r#"[{{ "left": [{CWD}, "jobs", {USAGE}] }}]"#),
    );

    let prompt = &rows[0];
    assert!(prompt.contains(" ~"), "{prompt:?}");
    assert!(prompt.contains(" \u{f085} 2 "), "{prompt:?}");
    assert!(prompt.contains(" \u{ec82} 5h 12% "), "{prompt:?}");
}

#[test]
fn replaced_icons_keep_the_right_prompt_aligned() {
    let scratch = Scratch::new("width");
    let rows = scratch.render(
        r#"{
            "jobs": { "icon": "日本" },
            "ai_usage": { "claude_icon": "" },
            "os": { "symbol": "" }
        }"#,
        &format!(r#"[{{ "left": ["jobs"], "right": [{USAGE}, "os"] }}]"#),
    );

    let prompt = &rows[0];
    assert!(prompt.contains(" 日本 2 "), "{prompt:?}");
    assert!(prompt.contains(" 5h 12% "), "{prompt:?}");
    // The padding fills the line to one column short of the terminal width.
    assert_eq!(prompt.width(), COLUMNS - 1, "{prompt:?}");
}
