use std::fs;
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

const BIN: &str = env!("CARGO_BIN_EXE_superline");

#[test]
fn usage_widget_renders_each_configured_provider_instance_from_cache() {
    let root = std::env::temp_dir().join(format!("superline-usage-it-{}", std::process::id()));
    let cache_dir = root.join("cache/superline");
    fs::create_dir_all(&cache_dir).expect("create cache directory");
    let fetched_at = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("current time")
        .as_secs();
    fs::write(
        cache_dir.join("usage-claude.json"),
        format!(
            r#"{{"fetched_at":{fetched_at},"value":{{"session":12.4,"weekly":67.8,"fable":33.3,"credits":{{"used":12.5,"limit":500.0,"unit":"dollars"}}}}}}"#
        ),
    )
    .expect("write Claude cache");
    fs::write(
        cache_dir.join("usage-codex.json"),
        format!(
            r#"{{"fetched_at":{fetched_at},"value":{{"session":100.0,"weekly":80.0,"credits":{{"used":410.78,"limit":12000.0,"unit":"count"}}}}}}"#
        ),
    )
    .expect("write Codex cache");

    fs::write(
        root.join("theme.json"),
        r#"{"defaults":{"fg":250,"bg":0},"modules":{"ai_usage":{"threshold_bg":203}}}"#,
    )
    .expect("write theme");
    let config = root.join("config.json");
    fs::write(
        &config,
        r#"{
            "theme": "theme.json",
            "rows": [{
                "left": [
                    {"ai_usage":{"provider":"claude","weekly":false,"display":"sparkline","session_label":""}},
                    {"ai_usage":{"provider":"codex","session":false,"display":"bar","threshold":75}},
                    {"ai_usage":{"provider":"claude","session":false,"weekly":false,"fable":true}},
                    {"ai_usage":{"provider":"claude","session":false,"weekly":false,"credits":true,"credits_display":"numeric","credits_only_when_limited":true}},
                    {"ai_usage":{"provider":"codex","session":false,"weekly":false,"credits":true,"credits_display":"numeric","credits_label":"","credits_only_when_limited":true}}
                ]
            }]
        }"#,
    )
    .expect("write config");

    let output = Command::new(BIN)
        .args(["show", "fish", "-s", "0", "-c", "120", "--config"])
        .arg(&config)
        .env("HOME", &root)
        .env("USERPROFILE", &root)
        .env("XDG_CACHE_HOME", root.join("cache"))
        .env("LOCALAPPDATA", root.join("cache"))
        .output()
        .expect("render prompt");

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    // The sparkline and bar widgets get a hover note on their icon, listing
    // windows they don't draw too. The percentage ones print the figures.
    assert!(
        stdout.contains(
            "\x1b]1337;AddHiddenAnnotation=1|5h: 12% used · 7d: 68% used · Fable: 33% used\x07\u{ec82} ▁"
        ),
        "stdout:\n{stdout}"
    );
    assert!(
        stdout.contains("\x1b]1337;AddHiddenAnnotation=1|5h: 100% used · 7d: 80% used\x07\u{ec81}"),
        "stdout:\n{stdout}"
    );
    assert_eq!(
        stdout.matches("AddHiddenAnnotation").count(),
        2,
        "stdout:\n{stdout}"
    );
    assert!(stdout.contains("\u{ec82}  F 33%"), "stdout:\n{stdout}");
    // Claude has headroom, so its hidden-until-limited credits lane stays empty.
    assert!(!stdout.contains("$12.50"), "stdout:\n{stdout}");
    // Codex has exhausted its session window, so its credits appear numerically.
    assert!(stdout.contains("\u{ec81} 411/12000"), "stdout:\n{stdout}");
    let warning_background = stdout
        .find("\x1b[48;5;203m")
        .expect("usage warning background should be rendered");
    let codex_icon = stdout
        .find("\u{ec81}")
        .expect("Codex icon should be rendered");
    assert!(
        warning_background < codex_icon,
        "the warning background should begin before the entire widget: {stdout}"
    );

    let _ = fs::remove_dir_all(root);
}

#[test]
fn usage_widget_separates_a_missing_provider_cli_from_a_pending_first_refresh() {
    let root = std::env::temp_dir().join(format!("superline-usage-state-{}", std::process::id()));
    let cache_dir = root.join("cache/superline");
    fs::create_dir_all(&cache_dir).expect("create cache directory");

    // Only Claude is on `PATH`. `resolve_binary` just looks for the file, so an
    // empty stub is enough - and the widget must not run it to decide what to
    // show.
    let path_dir = root.join("bin");
    fs::create_dir_all(&path_dir).expect("create PATH directory");
    let stub = if cfg!(windows) {
        "claude.exe"
    } else {
        "claude"
    };
    fs::write(path_dir.join(stub), b"").expect("write Claude stub");

    // Claim Claude's refresh slot so rendering doesn't spawn a background
    // refresh against the stub.
    let now_millis = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("current time")
        .as_millis();
    fs::write(
        cache_dir.join("usage-claude.refresh"),
        now_millis.to_string(),
    )
    .expect("claim the Claude refresh slot");

    let config = root.join("config.json");
    fs::write(
        &config,
        r#"{
            "rows": [{
                "left": [
                    {"ai_usage":{"provider":"claude"}},
                    {"ai_usage":{"provider":"codex"}}
                ]
            }]
        }"#,
    )
    .expect("write config");

    let output = Command::new(BIN)
        .args(["show", "fish", "-s", "0", "-c", "120", "--config"])
        .arg(&config)
        .env("HOME", &root)
        .env("USERPROFILE", &root)
        .env("XDG_CACHE_HOME", root.join("cache"))
        .env("LOCALAPPDATA", root.join("cache"))
        .env("PATH", &path_dir)
        .output()
        .expect("render prompt");

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    // Claude is installed with no reading yet, so its first refresh is pending.
    assert!(stdout.contains("\u{ec82} \u{2026}"), "stdout:\n{stdout}");
    // Codex is not installed, which is a different thing to say.
    assert!(stdout.contains("\u{ec81} ?"), "stdout:\n{stdout}");
    // A refresh that could only fail is not worth spawning.
    assert!(
        !cache_dir.join("usage-codex.refresh").exists(),
        "no Codex refresh should have been attempted"
    );

    let _ = fs::remove_dir_all(root);
}

#[test]
fn usage_widget_shows_a_logged_out_provider_instead_of_loading() {
    let root =
        std::env::temp_dir().join(format!("superline-usage-logged-out-{}", std::process::id()));
    let cache_dir = root.join("cache/superline");
    fs::create_dir_all(&cache_dir).expect("create cache directory");
    let fetched_at = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("current time")
        .as_secs();
    // What a refresh writes after `claude auth status` reports no account.
    fs::write(
        cache_dir.join("usage-claude.json"),
        format!(
            r#"{{"fetched_at":{fetched_at},"value":{{"session":null,"weekly":null,"logged_out":true}}}}"#
        ),
    )
    .expect("write Claude cache");

    let config = root.join("config.json");
    fs::write(
        &config,
        r#"{"theme":"rainbow","rows": [{"left": [{"ai_usage":{"provider":"claude","threshold":0}}]}]}"#,
    )
    .expect("write config");

    let output = Command::new(BIN)
        .args(["show", "fish", "-s", "0", "-c", "120", "--config"])
        .arg(&config)
        .env("HOME", &root)
        .env("USERPROFILE", &root)
        .env("XDG_CACHE_HOME", root.join("cache"))
        .env("LOCALAPPDATA", root.join("cache"))
        .output()
        .expect("render prompt");

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("\u{ec82} \u{f235}"), "stdout:\n{stdout}");
    assert!(!stdout.contains('\u{2026}'), "stdout:\n{stdout}");
    // A logged-out reading has nothing to measure against the threshold.
    assert!(!stdout.contains("\x1b[48;5;160m"), "stdout:\n{stdout}");

    let _ = fs::remove_dir_all(root);
}

/// Renders `config` for `shell` against a temporary cache holding `claude`
/// as the Claude reading, with `theme` as the theme file.
fn render_usage(name: &str, shell: &str, claude: &str, theme: &str, config: &str) -> String {
    let root = std::env::temp_dir().join(format!("superline-usage-{name}-{}", std::process::id()));
    let cache_dir = root.join("cache/superline");
    fs::create_dir_all(&cache_dir).expect("create cache directory");
    let fetched_at = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("current time")
        .as_secs();
    fs::write(
        cache_dir.join("usage-claude.json"),
        format!(r#"{{"fetched_at":{fetched_at},"value":{claude}}}"#),
    )
    .expect("write Claude cache");
    fs::write(root.join("theme.json"), theme).expect("write theme");
    let config_path = root.join("config.json");
    fs::write(
        &config_path,
        format!(r#"{{"theme":"theme.json","rows":[{config}]}}"#),
    )
    .expect("write config");

    let output = Command::new(BIN)
        .args(["show", shell, "-s", "0", "-c", "120", "--config"])
        .arg(&config_path)
        .env("HOME", &root)
        .env("USERPROFILE", &root)
        .env("XDG_CACHE_HOME", root.join("cache"))
        .env("LOCALAPPDATA", root.join("cache"))
        .output()
        .expect("render prompt");
    let _ = fs::remove_dir_all(root);
    assert!(output.status.success(), "{name} {shell}");
    String::from_utf8_lossy(&output.stdout).into_owned()
}

const READING: &str = r#"{"session":61.0,"weekly":41.0,"fable":22.0}"#;
const THEME: &str = r#"{"defaults":{"fg":250,"bg":0},"modules":{}}"#;
const NOTE: &str = "5h: 61% used · 7d: 41% used · Fable: 22% used";

fn widget(options: &str) -> String {
    format!(r#"{{"left":[{{"ai_usage":{{"provider":"claude",{options}}}}}]}}"#)
}

#[test]
fn usage_hover_note_covers_the_icon_in_each_shell() {
    let config = widget(r#""display":"sparkline""#);
    for (shell, note) in [
        (
            "fish",
            format!("\x1b]1337;AddHiddenAnnotation=1|{NOTE}\x07\u{ec82} 5h ▅"),
        ),
        (
            "zsh",
            format!(
                "%{{\x1b]1337;AddHiddenAnnotation=1|{}\x07%}}\u{ec82} 5h ▅",
                NOTE.replace('%', "%%")
            ),
        ),
        (
            "bash",
            format!(
                r"\[\e]1337;AddHiddenAnnotation=1|{NOTE}\a\]{}",
                "\u{ec82} 5h ▅"
            ),
        ),
    ] {
        let stdout = render_usage("hover-shells", shell, READING, THEME, &config);
        assert!(stdout.contains(&note), "{shell} stdout:\n{stdout}");
        assert_eq!(
            stdout.matches("AddHiddenAnnotation").count(),
            1,
            "{shell} stdout:\n{stdout}"
        );
    }
}

#[test]
fn usage_hover_note_is_only_drawn_where_the_figures_are_hidden() {
    let note = format!("\x1b]1337;AddHiddenAnnotation=1|{NOTE}\x07\u{ec82}");
    for display in ["bar", "capped_bar", "block", "sparkline"] {
        // Only the session lane is drawn, but every reading is listed.
        let config = widget(&format!(r#""display":"{display}","weekly":false"#));
        let stdout = render_usage("hover-displays", "fish", READING, THEME, &config);
        assert!(stdout.contains(&note), "{display} stdout:\n{stdout}");
    }

    let no_icon = r#"{"defaults":{"fg":250,"bg":0},"modules":{"ai_usage":{"claude_icon":""}}}"#;
    let no_readings = r#"{"session":null,"weekly":null}"#;
    for (case, reading, theme, options) in [
        ("percentage", READING, THEME, r#""display":"percentage""#),
        ("numeric", READING, THEME, r#""display":"numeric""#),
        (
            "hover off",
            READING,
            THEME,
            r#""display":"sparkline","hover":false"#,
        ),
        ("no icon", READING, no_icon, r#""display":"sparkline""#),
        (
            "no readings",
            no_readings,
            THEME,
            r#""display":"sparkline""#,
        ),
    ] {
        let stdout = render_usage("hover-none", "fish", reading, theme, &widget(options));
        assert!(stdout.contains("5h"), "{case} stdout:\n{stdout}");
        assert!(
            !stdout.contains("AddHiddenAnnotation"),
            "{case} stdout:\n{stdout}"
        );
    }
}

/// The text a terminal shows for `prompt`, without its escapes.
fn visible(prompt: &str) -> String {
    let mut out = String::new();
    let mut chars = prompt.chars();
    while let Some(c) = chars.next() {
        match c {
            '\x1b' => match chars.next() {
                Some('[') => {
                    chars.find(|c| c.is_ascii_alphabetic());
                }
                Some(']') => {
                    chars.find(|&c| c == '\x07');
                }
                _ => {}
            },
            c => out.push(c),
        }
    }
    out
}

#[test]
fn usage_hover_note_takes_no_columns() {
    // Every row but the last is padded out to the right side by superline.
    let rows = |hover: bool| {
        format!(
            r#"{{"left":[{{"ai_usage":{{"provider":"claude","display":"sparkline","hover":{hover}}}}}],"right":[{{"text":"end"}}]}},{{"left":[{{"text":"next"}}]}}"#
        )
    };
    let with_note = render_usage("hover-width", "fish", READING, THEME, &rows(true));
    let without = render_usage("hover-width", "fish", READING, THEME, &rows(false));
    assert!(with_note.contains("AddHiddenAnnotation"), "{with_note}");
    assert_ne!(with_note, without);
    assert_eq!(visible(&with_note), visible(&without));
    let first_row = visible(&with_note)
        .lines()
        .next()
        .unwrap_or_default()
        .to_string();
    assert!(first_row.ends_with("end "), "{first_row:?}");
    // 120 columns, less the one superline keeps free.
    assert_eq!(first_row.chars().count(), 119, "{first_row:?}");
}
