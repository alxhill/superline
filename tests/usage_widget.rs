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
        format!(r#"{{"session":12.4,"weekly":67.8,"fetched_at":{fetched_at}}}"#),
    )
    .expect("write Claude cache");
    fs::write(
        cache_dir.join("usage-codex.json"),
        format!(r#"{{"session":40.0,"weekly":80.0,"fetched_at":{fetched_at}}}"#),
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
                    {"ai_usage":{"provider":"codex","session":false,"display":"bar","threshold":75}}
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
