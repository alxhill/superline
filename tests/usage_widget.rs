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
            r#"{{"session":12.4,"weekly":67.8,"fable":33.3,"credits":{{"used":12.5,"limit":500.0,"unit":"dollars"}},"fetched_at":{fetched_at}}}"#
        ),
    )
    .expect("write Claude cache");
    fs::write(
        cache_dir.join("usage-codex.json"),
        format!(
            r#"{{"session":100.0,"weekly":80.0,"credits":{{"used":410.78,"limit":12000.0,"unit":"count"}},"fetched_at":{fetched_at}}}"#
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
    assert!(stdout.contains("\u{ec82} ▂"), "stdout:\n{stdout}");
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
        format!(r#"{{"session":null,"weekly":null,"logged_out":true,"fetched_at":{fetched_at}}}"#),
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
