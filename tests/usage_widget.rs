//! End-to-end checks for the `ai_usage` widget against the metadata server.
//!
//! The server normally fetches Claude/Codex usage through their interactive
//! CLIs, which isn't something a test can fake easily. These tests instead
//! start the real server with a `SUPERLINE_METADATA_FIXTURE` snapshot and then
//! drive the real `show` binary, so the full client -> server -> render path is
//! exercised with controlled readings.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

const BIN: &str = env!("CARGO_BIN_EXE_superline");

fn scratch_root(label: &str) -> PathBuf {
    let root = std::env::temp_dir().join(format!(
        "superline-usage-server-{}-{label}",
        std::process::id()
    ));
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(root.join("cache/superline")).expect("create cache directory");
    root
}

/// Cache directory under `root`, set as both XDG_CACHE_HOME (Unix) and
/// LOCALAPPDATA (Windows) so the server and client agree on every platform.
fn cache_env(root: &Path) -> PathBuf {
    root.join("cache")
}

fn start_server(root: &Path, fixture: &Path) -> Child {
    let child = Command::new(BIN)
        .arg("server")
        .env("HOME", root)
        .env("USERPROFILE", root)
        .env("XDG_CACHE_HOME", cache_env(root))
        .env("LOCALAPPDATA", cache_env(root))
        .env("SUPERLINE_METADATA_FIXTURE", fixture)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("start metadata server");

    // The server writes its control file right after binding the socket.
    let control = cache_env(root).join("superline/server.json");
    let deadline = Instant::now() + Duration::from_secs(5);
    while !control.exists() {
        assert!(
            Instant::now() < deadline,
            "server did not publish its control file"
        );
        thread::sleep(Duration::from_millis(10));
    }

    child
}

fn stop_server(mut child: Child) {
    let _ = child.kill();
    let _ = child.wait();
}

fn run_show(root: &Path, config: &Path) -> String {
    let output = Command::new(BIN)
        .args(["show", "fish", "-s", "0", "-c", "120", "--config"])
        .arg(config)
        .env("HOME", root)
        .env("USERPROFILE", root)
        .env("XDG_CACHE_HOME", cache_env(root))
        .env("LOCALAPPDATA", cache_env(root))
        .output()
        .expect("render prompt");

    assert!(
        output.status.success(),
        "show exited with failure\nstderr:\n{}",
        String::from_utf8_lossy(&output.stderr),
    );
    String::from_utf8_lossy(&output.stdout).into_owned()
}

fn fetched_at() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("current time")
        .as_secs()
}

fn write_metadata(root: &Path, metadata: &str) -> PathBuf {
    let fixture = root.join("metadata.json");
    fs::write(&fixture, metadata).expect("write metadata fixture");
    fixture
}

#[test]
fn usage_widget_renders_each_configured_provider_instance_from_server() {
    let root = scratch_root("values");
    let fetched_at = fetched_at();
    let fixture = write_metadata(
        &root,
        &format!(
            r#"{{
                "git": "Pending",
                "pr": "Pending",
                "usage_claude": {{"Ready":{{"session":12.4,"weekly":67.8,"fable":33.3,"credits":{{"used":12.5,"limit":500.0,"unit":"dollars"}},"logged_out":false,"fetched_at":{fetched_at}}}}},
                "usage_codex": {{"Ready":{{"session":100.0,"weekly":80.0,"credits":{{"used":410.78,"limit":12000.0,"unit":"count"}},"logged_out":false,"fetched_at":{fetched_at}}}}}
            }}"#
        ),
    );

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

    let server = start_server(&root, &fixture);
    let stdout = run_show(&root, &config);
    stop_server(server);

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

    let _ = fs::remove_dir_all(&root);
}

#[test]
fn usage_widget_separates_a_pending_first_refresh_from_a_missing_provider() {
    let root = scratch_root("pending-vs-missing");
    let fixture = write_metadata(
        &root,
        r#"{
            "git": "Pending",
            "pr": "Pending",
            "usage_claude": "Pending",
            "usage_codex": "Unavailable"
        }"#,
    );

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

    let server = start_server(&root, &fixture);
    let stdout = run_show(&root, &config);
    stop_server(server);

    // Claude has no reading yet, so its first refresh is pending.
    assert!(stdout.contains("\u{ec82} \u{2026}"), "stdout:\n{stdout}");
    // Codex is not installed, which is a different thing to say.
    assert!(stdout.contains("\u{ec81} ?"), "stdout:\n{stdout}");

    let _ = fs::remove_dir_all(&root);
}

#[test]
fn usage_widget_shows_a_logged_out_provider_instead_of_loading() {
    let root = scratch_root("logged-out");
    let fetched_at = fetched_at();
    let fixture = write_metadata(
        &root,
        &format!(
            r#"{{
                "git": "Pending",
                "pr": "Pending",
                "usage_claude": {{"Ready":{{"session":null,"weekly":null,"logged_out":true,"fetched_at":{fetched_at}}}}},
                "usage_codex": "Unavailable"
            }}"#
        ),
    );

    let config = root.join("config.json");
    fs::write(
        &config,
        r#"{"theme":"rainbow","rows": [{"left": [{"ai_usage":{"provider":"claude","threshold":0}}]}]}"#,
    )
    .expect("write config");

    let server = start_server(&root, &fixture);
    let stdout = run_show(&root, &config);
    stop_server(server);

    assert!(stdout.contains("\u{ec82} \u{f235}"), "stdout:\n{stdout}");
    assert!(!stdout.contains('\u{2026}'), "stdout:\n{stdout}");
    // A logged-out reading has nothing to measure against the threshold.
    assert!(!stdout.contains("\x1b[48;5;160m"), "stdout:\n{stdout}");

    let _ = fs::remove_dir_all(&root);
}
