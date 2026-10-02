//! End-to-end checks for the git widget's `remote_link` option.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use serde_json::{json, Value};

const BIN: &str = env!("CARGO_BIN_EXE_superline");
const REPO_URL: &str = "https://github.com/example/repo";

/// A repository at `<root>/repo` whose `origin` points at GitHub, one commit
/// ahead of its upstream, and an empty scratch home.
fn scratch(label: &str) -> PathBuf {
    let root = std::env::temp_dir().join(format!(
        "superline-git-remote-link-{}-{label}",
        std::process::id()
    ));
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(root.join("home/.config/superline")).expect("create config dir");

    let repo = root.join("repo");
    fs::create_dir_all(&repo).expect("create repo dir");
    git(&repo, &["init", "-q", "-b", "main"]);
    git(&repo, &["config", "user.email", "test@example.com"]);
    git(&repo, &["config", "user.name", "test"]);
    git(&repo, &["commit", "-q", "--allow-empty", "-m", "init"]);
    git(
        &repo,
        &["remote", "add", "origin", &format!("{REPO_URL}.git")],
    );
    git(&repo, &["update-ref", "refs/remotes/origin/main", "HEAD"]);
    git(&repo, &["branch", "-q", "--set-upstream-to=origin/main"]);
    git(&repo, &["commit", "-q", "--allow-empty", "-m", "ahead"]);
    root
}

fn git(dir: &Path, args: &[&str]) {
    let status = Command::new("git")
        .current_dir(dir)
        .args(args)
        .status()
        .expect("run git");
    assert!(status.success(), "`git {}` failed", args.join(" "));
}

/// Renders a prompt holding only a git segment with `options` from the repo.
fn render(root: &Path, options: Value) -> String {
    let home = root.join("home");
    let config_dir = home.join(".config/superline");
    let theme = json!({
        "defaults": { "fg": 15, "bg": 0 },
        "modules": { "git": { "remote_icon": "R", "ahead_icon": "^", "behind_icon": "v" } },
    });
    fs::write(config_dir.join("theme.json"), theme.to_string()).expect("write theme");
    let mut options = options;
    options["status_timeout_ms"] = json!(30_000);
    let config = json!({
        "theme": "theme.json",
        "update": { "disable": true },
        "rows": [{ "left": [{ "git": options }] }],
    });
    fs::write(config_dir.join("config.json"), config.to_string()).expect("write config");

    let cache = root.join("cache");
    let output = Command::new(BIN)
        .args(["show", "fish", "-s", "0", "-c", "120"])
        .current_dir(root.join("repo"))
        .env("HOME", &home)
        .env("USERPROFILE", &home)
        .env("XDG_CACHE_HOME", &cache)
        .env("LOCALAPPDATA", &cache)
        .output()
        .expect("run superline");
    assert!(
        output.status.success(),
        "show failed:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout).into_owned()
}

#[test]
fn the_remote_segment_links_to_the_repository_by_default() {
    let root = scratch("on");
    for options in [json!({}), json!({ "remote_link": true })] {
        let prompt = render(&root, options);
        assert!(
            prompt.contains(&format!("\x1b]8;;{REPO_URL}\x1b\\R 1^ \x1b]8;;\x1b\\")),
            "prompt: {prompt:?}"
        );
    }
    let _ = fs::remove_dir_all(&root);
}

#[test]
fn turning_remote_link_off_keeps_only_the_counts() {
    let root = scratch("off");
    let prompt = render(&root, json!({ "remote_link": false }));
    assert!(prompt.contains("1^"), "prompt: {prompt:?}");
    assert!(!prompt.contains("R 1^"), "prompt: {prompt:?}");
    assert!(!prompt.contains(REPO_URL), "prompt: {prompt:?}");
    assert!(!prompt.contains("\x1b]8;;"), "prompt: {prompt:?}");
    let _ = fs::remove_dir_all(&root);
}
