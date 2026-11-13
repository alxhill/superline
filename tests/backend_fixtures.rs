//! End-to-end checks that `SUPERLINE_FIXTURES` stands in for every backend a
//! widget reads outside the shell integration, without touching the cache,
//! the network or any CLI.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use serde_json::{json, Value};

const BIN: &str = env!("CARGO_BIN_EXE_superline");

struct Scratch {
    root: PathBuf,
}

impl Scratch {
    /// An empty home, and a "repository" that is nothing but `.git/HEAD` on
    /// `branch`, so no `git` binary is involved.
    fn new(label: &str, branch: &str) -> Scratch {
        let root =
            std::env::temp_dir().join(format!("superline-fixtures-{}-{label}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(root.join("home/.config/superline")).unwrap();
        fs::create_dir_all(root.join("repo/.git")).unwrap();
        fs::write(
            root.join("repo/.git/HEAD"),
            format!("ref: refs/heads/{branch}\n"),
        )
        .unwrap();
        Scratch { root }
    }

    fn cache(&self) -> PathBuf {
        self.root.join("cache")
    }

    fn render(&self, segments: Value, fixtures: &Value) -> Output {
        let home = self.root.join("home");
        let config = json!({ "theme": "rainbow", "rows": [{ "left": segments }] });
        fs::write(
            home.join(".config/superline/config.json"),
            config.to_string(),
        )
        .unwrap();
        let fixtures_path = self.root.join("fixtures.json");
        fs::write(&fixtures_path, fixtures.to_string()).unwrap();

        Command::new(BIN)
            .args(["show", "fish", "-s", "0", "-c", "300"])
            .current_dir(self.root.join("repo"))
            .env("HOME", &home)
            .env("USERPROFILE", &home)
            .env("XDG_CONFIG_HOME", home.join(".config"))
            .env("XDG_CACHE_HOME", self.cache())
            .env("LOCALAPPDATA", self.cache())
            .env("TZ", "UTC")
            .env("VIRTUAL_ENV", self.root.join("venv"))
            .env("KUBECONFIG", self.root.join("kubeconfig"))
            // Nothing on PATH: no git, gh, claude, codex or sudo to fall back on.
            .env("PATH", self.root.join("empty-path"))
            .env("SUPERLINE_FIXTURES", &fixtures_path)
            .output()
            .expect("run superline")
    }

    fn prompt(&self, segments: Value, fixtures: &Value) -> String {
        let output = self.render(segments, fixtures);
        assert!(
            output.status.success(),
            "show failed:\n{}",
            String::from_utf8_lossy(&output.stderr)
        );
        plain(&String::from_utf8_lossy(&output.stdout))
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

/// The prompt's text without colours, hyperlinks or fish's escapes.
fn plain(prompt: &str) -> String {
    let mut out = String::new();
    let mut chars = prompt.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '\x1b' => match chars.next() {
                Some('[') => {
                    for c in chars.by_ref() {
                        if c.is_ascii_alphabetic() {
                            break;
                        }
                    }
                }
                Some(']') => {
                    while let Some(c) = chars.next() {
                        if c == '\x07' || (c == '\x1b' && chars.next_if_eq(&'\\').is_some()) {
                            break;
                        }
                    }
                }
                _ => {}
            },
            _ => out.push(c),
        }
    }
    out
}

fn assert_contains(prompt: &str, wanted: &[&str]) {
    for text in wanted {
        assert!(prompt.contains(text), "missing {text:?} in {prompt:?}");
    }
}

fn assert_no_cache(scratch: &Scratch) {
    assert!(
        !scratch.cache().exists(),
        "fixtured widgets wrote to the cache: {:?}",
        fs::read_dir(scratch.cache())
            .map(|dir| dir.flatten().map(|e| e.file_name()).collect::<Vec<_>>())
    );
}

#[test]
fn machine_probes_come_from_fixtures() {
    let scratch = Scratch::new("machine", "main");
    let prompt = scratch.prompt(
        json!([
            "user",
            "hostname",
            "local_ip",
            "battery",
            "memory_usage",
            { "time": { "format": "%H:%M" } },
            "read_only",
            "sudo"
        ]),
        &json!({
            "clock": "2026-01-01T09:41:00Z",
            "username": "demo",
            "root": false,
            "hostname": "devbox",
            "local_ip": "10.1.2.3",
            "battery": { "percentage": 7.0, "state": "discharging" },
            "memory": { "total": 100, "available": 20, "swap_total": 10, "swap_available": 5 },
            "read_only": true,
            "sudo": true
        }),
    );
    assert_contains(
        &prompt,
        &[
            "demo",
            "devbox",
            "10.1.2.3",
            "7%",
            "80% | swap 50%",
            "09:41",
        ],
    );
    assert_no_cache(&scratch);
}

#[test]
fn null_probes_hide_their_widget() {
    let scratch = Scratch::new("null", "main");
    let prompt = scratch.prompt(
        json!(["hostname", "battery", "os"]),
        &json!({ "hostname": null, "battery": null }),
    );
    assert!(!prompt.contains('%'), "{prompt:?}");
}

#[test]
fn repository_lookups_come_from_fixtures() {
    let scratch = Scratch::new("repo", "feat/login");
    let segments = json!([
        "git",
        "diff",
        { "pr": { "status": true, "review": true } },
        "pr_diff",
        "python",
        "kubernetes"
    ]);
    let fixtures = json!({
        "git": { "branch_name": "feat/login", "ahead": 2, "behind": 1, "staged": 1, "remote": true },
        "diff": { "added": 42, "removed": 7 },
        "pr": {
            "feat/login": {
                "number": 148,
                "url": "https://github.com/alxhill/superline/pull/148",
                "state": "open",
                "checks": "pending",
                "diff": { "additions": 444, "deletions": 37 }
            },
            "*": null
        },
        "python": "3.12.4",
        "kubernetes": { "name": "prod", "namespace": "web" }
    });
    let prompt = scratch.prompt(segments.clone(), &fixtures);
    assert_contains(
        &prompt,
        &[
            "feat/login",
            "+42 -7",
            "#148",
            "+444 -37",
            "3.12.4",
            "prod (web)",
        ],
    );
    assert_no_cache(&scratch);

    // Any other branch falls through to "*", which has no PR.
    let other = Scratch::new("repo-other", "main");
    let prompt = other.prompt(segments, &fixtures);
    assert!(!prompt.contains("#148"), "{prompt:?}");
}

#[test]
fn usage_is_keyed_by_provider_and_pinned_to_the_clock() {
    let scratch = Scratch::new("usage", "main");
    let prompt = scratch.prompt(
        json!([
            { "ai_usage": { "provider": "claude", "session_time_remaining": true } },
            { "ai_usage": { "provider": "codex" } }
        ]),
        &json!({
            "clock": "2026-01-01T09:00:00Z",
            "usage": {
                "claude": { "session": 38.0, "weekly": 61.0, "session_resets_at": 1_767_265_200 },
                "codex": { "session": 84.0, "weekly": 47.0 }
            }
        }),
    );
    assert_contains(&prompt, &["38%", "61%", "2h 0m", "84%", "47%"]);
    assert_no_cache(&scratch);
}

#[test]
fn a_lookup_can_be_held_loading() {
    let scratch = Scratch::new("loading", "main");
    let segments = json!([{ "ai_usage": { "provider": "claude" } }]);
    let loading = scratch.prompt(
        segments.clone(),
        &json!({ "usage": { "claude": { "$lookup": "loading" } } }),
    );
    let unavailable = scratch.prompt(
        segments,
        &json!({ "usage": { "claude": { "$lookup": "unavailable" } } }),
    );
    assert_ne!(loading, unavailable);
    assert!(!loading.contains('%'), "{loading:?}");
    assert_no_cache(&scratch);
}

#[test]
fn update_checks_never_run_under_fixtures() {
    let scratch = Scratch::new("update", "main");
    let prompt = scratch.prompt(json!(["os"]), &json!({ "os": "linux" }));
    assert!(!prompt.contains("superline"), "{prompt:?}");
    assert!(
        !scratch.cache().join("superline").exists() || {
            let names: Vec<_> = fs::read_dir(scratch.cache().join("superline"))
                .unwrap()
                .flatten()
                .map(|entry| entry.file_name().to_string_lossy().into_owned())
                .collect();
            !names
                .iter()
                .any(|name| name.starts_with("update") || name.starts_with("auto-upgrade"))
        }
    );
}

#[test]
fn a_fixture_that_does_not_parse_fails_the_prompt() {
    let scratch = Scratch::new("bad", "main");
    let output = scratch.render(json!(["hostname"]), &json!({ "hostname": 42 }));
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("fixture `hostname`"), "{stderr}");
    let prompt = plain(&String::from_utf8_lossy(&output.stdout));
    assert!(prompt.contains("fixture `hostname`"), "{prompt:?}");
}

#[test]
fn an_unknown_fixture_key_fails_the_prompt() {
    let scratch = Scratch::new("unknown", "main");
    let output = scratch.render(json!(["os"]), &json!({ "hostnme": "devbox" }));
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("fixture `hostnme`: not a fixture key"),
        "{stderr}"
    );
}

#[test]
fn unreadable_fixtures_fail_the_prompt() {
    let scratch = Scratch::new("unreadable", "main");
    let output = Command::new(BIN)
        .args(["show", "fish", "-s", "0", "-c", "100"])
        .current_dir(scratch.root.join("repo"))
        .env("HOME", scratch.root.join("home"))
        .env("XDG_CACHE_HOME", scratch.cache())
        .env("SUPERLINE_FIXTURES", scratch.root.join("missing.json"))
        .output()
        .unwrap();
    assert!(!output.status.success());
}

/// `examples/fixtures.json` names every backend, so it doubles as the
/// reference for what each one looks like.
#[test]
fn the_example_fixtures_parse() {
    let example = Path::new(env!("CARGO_MANIFEST_DIR")).join("examples/fixtures.json");
    let fixtures: Value = serde_json::from_str(&fs::read_to_string(example).unwrap()).unwrap();
    let scratch = Scratch::new("example", "feat/login");
    let output = scratch.render(
        json!([
            "user", "hostname", "os", "local_ip", "battery", "memory_usage", "time",
            "read_only", "sudo", "git", "git_remote", "diff", "pr", "pr_checks", "pr_diff",
            "python", "kubernetes",
            { "ai_usage": { "provider": "claude" } },
            { "ai_usage": { "provider": "codex" } }
        ]),
        &fixtures,
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(output.status.success(), "{stderr}");
    assert_no_cache(&scratch);

    let mut keys: Vec<&str> = fixtures
        .as_object()
        .unwrap()
        .keys()
        .map(String::as_str)
        .collect();
    let mut known = superline::backend::KEYS.to_vec();
    keys.sort_unstable();
    known.sort_unstable();
    assert_eq!(keys, known, "examples/fixtures.json should name every key");
}
