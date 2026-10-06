//! Theme text attributes (`bold`, `italic`, `underline`) in each shell's
//! prompt: wrapped like the colour escapes, and never left on past the text
//! they style.

use std::collections::BTreeSet;
use std::fs;
use std::process::Command;

use serde_json::{json, Value};
use superline::colors::{Color, TextAttrs};
use superline::modules::*;
use superline::terminal::{Shell, SHELL};
use superline::themes::CustomTheme;
use superline::update::UpdateScheme;
use superline::Style;

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

type T = CustomTheme;

/// A module, a text colour property it reads, and the getter it draws with.
type TextColor = (&'static str, &'static str, fn() -> Color);

/// Every text colour a module draws with, by the theme property it reads.
const TEXT_COLORS: &[TextColor] = &[
    ("cwd", "path_fg", <T as CwdScheme>::path_fg),
    ("readonly", "fg", <T as ReadOnlyScheme>::readonly_fg),
    ("cmd", "passed_fg", <T as CmdScheme>::cmd_passed_fg),
    ("cmd", "failed_fg", <T as CmdScheme>::cmd_failed_fg),
    (
        "last_cmd_duration",
        "fg",
        <T as LastCmdDurationScheme>::time_fg,
    ),
    ("shell", "fg", <T as ShellScheme>::shellname_fg),
    ("jobs", "fg", <T as JobsScheme>::jobs_fg),
    ("git", "clean_fg", <T as GitScheme>::git_repo_clean_fg),
    ("git", "dirty_fg", <T as GitScheme>::git_repo_dirty_fg),
    ("git", "notstaged_fg", <T as GitScheme>::git_notstaged_fg),
    ("git", "untracked_fg", <T as GitScheme>::git_untracked_fg),
    ("git", "staged_fg", <T as GitScheme>::git_staged_fg),
    ("git", "conflicted_fg", <T as GitScheme>::git_conflicted_fg),
    ("git", "remote_fg", <T as GitScheme>::git_remote_fg),
    ("pr", "draft_fg", <T as PrScheme>::pr_draft_fg),
    ("pr", "open_fg", <T as PrScheme>::pr_open_fg),
    ("pr", "merged_fg", <T as PrScheme>::pr_merged_fg),
    ("pr", "closed_fg", <T as PrScheme>::pr_closed_fg),
    (
        "pr",
        "status_success_fg",
        <T as PrScheme>::pr_status_success_fg,
    ),
    (
        "pr",
        "status_failure_fg",
        <T as PrScheme>::pr_status_failure_fg,
    ),
    (
        "pr",
        "status_pending_fg",
        <T as PrScheme>::pr_status_pending_fg,
    ),
    (
        "pr",
        "review_pending_fg",
        <T as PrScheme>::pr_review_pending_fg,
    ),
    (
        "pr",
        "review_commented_fg",
        <T as PrScheme>::pr_review_commented_fg,
    ),
    (
        "pr",
        "review_changes_requested_fg",
        <T as PrScheme>::pr_review_changes_requested_fg,
    ),
    (
        "pr",
        "review_approved_fg",
        <T as PrScheme>::pr_review_approved_fg,
    ),
    ("pr_diff", "added_fg", <T as PrScheme>::pr_diff_added_fg),
    ("pr_diff", "removed_fg", <T as PrScheme>::pr_diff_removed_fg),
    ("python", "env_fg", <T as PythonScheme>::pyenv_fg),
    ("python", "version_fg", <T as PythonScheme>::pyver_fg),
    ("node", "fg", <T as NodeScheme>::node_fg),
    ("java", "fg", <T as JavaScheme>::java_fg),
    ("cargo", "fg", <T as CargoScheme>::cargo_fg),
    ("ai_usage", "claude_fg", <T as UsageScheme>::claude_usage_fg),
    ("ai_usage", "codex_fg", <T as UsageScheme>::codex_usage_fg),
    (
        "claude_model",
        "fg",
        <T as ClaudeCodeScheme>::claude_model_fg,
    ),
    (
        "claude_model",
        "effort_fg",
        <T as ClaudeCodeScheme>::claude_model_effort_fg,
    ),
    (
        "claude_context",
        "fg",
        <T as ClaudeCodeScheme>::claude_context_fg,
    ),
    ("claude_cost", "fg", <T as ClaudeCodeScheme>::claude_cost_fg),
    (
        "claude_duration",
        "fg",
        <T as ClaudeCodeScheme>::claude_duration_fg,
    ),
    ("diff", "added_fg", <T as DiffScheme>::diff_added_fg),
    ("diff", "removed_fg", <T as DiffScheme>::diff_removed_fg),
    (
        "claude_cache",
        "fg",
        <T as ClaudeCodeScheme>::claude_cache_fg,
    ),
    ("claude_vim", "fg", <T as ClaudeCodeScheme>::claude_vim_fg),
    (
        "claude_agent",
        "fg",
        <T as ClaudeCodeScheme>::claude_agent_fg,
    ),
    (
        "claude_session",
        "fg",
        <T as ClaudeCodeScheme>::claude_session_fg,
    ),
    ("os", "fg", <T as OsScheme>::os_fg),
    (
        "memory_usage",
        "fg",
        <T as MemoryUsageScheme>::memory_usage_fg,
    ),
    ("time", "fg", <T as TimeScheme>::time_fg),
    ("username", "fg", <T as UserScheme>::username_fg),
    ("hostname", "fg", <T as HostScheme>::hostname_fg),
    ("local_ip", "fg", <T as LocalIpScheme>::local_ip_fg),
    ("battery", "fg", <T as BatteryScheme>::battery_fg),
    ("sudo", "fg", <T as SudoScheme>::sudo_fg),
    ("kubernetes", "fg", <T as KubernetesScheme>::kubernetes_fg),
    ("spacer", "fg", <T as SpacerScheme>::color_fg),
    ("update", "fg", <T as UpdateScheme>::update_fg),
    ("error", "fg", <T as ErrorMessageScheme>::error_message_fg),
    ("unknown", "fg", <T as UnknownScheme>::unknown_fg),
    ("exit_code", "fg", <T as ExitCodeScheme>::exit_code_fg),
];

/// Every combination of attributes but none, handed out in turn so the
/// colours of one module each get a different one.
fn attrs_for(row: usize) -> TextAttrs {
    let bits = row % 7 + 1;
    TextAttrs {
        bold: bits & 1 != 0,
        italic: bits & 2 != 0,
        underline: bits & 4 != 0,
    }
}

/// The attribute property for `fg_property`, e.g. `clean_bold`.
fn attribute_key(fg_property: &str, attr: &str) -> String {
    match fg_property.strip_suffix("fg") {
        Some("") => attr.to_string(),
        Some(prefix) => format!("{prefix}{attr}"),
        None => panic!("{fg_property} is not a text colour"),
    }
}

#[test]
fn every_documented_text_color_is_drawn_with_its_attributes() {
    let options: Value = serde_json::from_str(include_str!(
        "../scripts/site-screenshots/theme-options.json"
    ))
    .unwrap();
    let documented: BTreeSet<(String, String)> = options
        .as_object()
        .unwrap()
        .iter()
        .filter(|(module, _)| !module.starts_with('_'))
        .flat_map(|(module, spec)| {
            spec["properties"]
                .as_array()
                .unwrap()
                .iter()
                .filter(|row| row[1] == "color")
                .filter_map(|row| row[0].as_str())
                .filter(|key| *key == "fg" || key.ends_with("_fg"))
                .map(|key| (module.clone(), key.to_string()))
        })
        .collect();
    let tested: BTreeSet<(String, String)> = TEXT_COLORS
        .iter()
        .map(|(module, key, _)| (module.to_string(), key.to_string()))
        .collect();
    let untested: Vec<_> = documented.difference(&tested).collect();
    assert!(untested.is_empty(), "no getter listed for {untested:?}");

    let mut modules = json!({});
    for (row, (module, fg, _)) in TEXT_COLORS.iter().enumerate() {
        let attrs = attrs_for(row);
        for (attr, on) in [
            ("bold", attrs.bold),
            ("italic", attrs.italic),
            ("underline", attrs.underline),
        ] {
            modules[*module][attribute_key(fg, attr)] = Value::Bool(on);
        }
    }
    let dir = std::env::temp_dir().join(format!("superline-attrs-{}-getters", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    let path = dir.join("theme.json");
    let theme = json!({ "defaults": { "fg": 15, "bg": 0 }, "modules": modules });
    fs::write(&path, theme.to_string()).unwrap();
    CustomTheme::load(&path).unwrap();
    let _ = fs::remove_dir_all(&dir);
    let _ = SHELL.set(Shell::Bare);

    for (row, (module, fg, getter)) in TEXT_COLORS.iter().enumerate() {
        let attrs = attrs_for(row);
        let color = getter();
        assert_eq!(color.attrs(), attrs, "{module}.{fg}");
        // Modules draw every segment through Style::simple.
        let drawn = Style::simple(color, Color(0)).fg.to_string();
        let expected = format!(
            "\x1b[38;5;{}m\x1b[{}m",
            color.code().palette().unwrap(),
            attrs.on_codes()
        );
        assert_eq!(drawn, expected, "{module}.{fg}");
    }
}
