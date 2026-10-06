//! End-to-end checks for the PR widgets, with `gh` answered by the site
//! screenshots' stub.
#![cfg(unix)]

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, Instant};

use serde_json::{json, Value};

const BIN: &str = env!("CARGO_BIN_EXE_superline");

/// A repository at `<root>/repo` on a branch the stub has an open PR for
/// (#142, 426 lines added and 35 deleted), and an empty scratch home.
fn scratch(label: &str) -> PathBuf {
    let root = std::env::temp_dir().join(format!(
        "superline-pr-widget-{}-{label}",
        std::process::id()
    ));
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(root.join("home/.config/superline")).expect("create config dir");

    let repo = root.join("repo");
    fs::create_dir_all(&repo).expect("create repo dir");
    git(&repo, &["init", "-q", "-b", "feat/usage-sparklines"]);
    git(&repo, &["config", "user.email", "test@example.com"]);
    git(&repo, &["config", "user.name", "test"]);
    git(&repo, &["commit", "-q", "--allow-empty", "-m", "init"]);
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

fn render(root: &Path, segments: Value) -> String {
    let home = root.join("home");
    let config = json!({
        "theme": "rainbow",
        "update": { "disable": true },
        "rows": [{ "left": segments }],
    });
    fs::write(
        home.join(".config/superline/config.json"),
        config.to_string(),
    )
    .expect("write config");

    let stub = Path::new(env!("CARGO_MANIFEST_DIR")).join("scripts/site-screenshots/bin");
    let path = std::env::join_paths(
        std::iter::once(stub).chain(std::env::split_paths(&std::env::var_os("PATH").unwrap())),
    )
    .unwrap();
    let cache = root.join("cache");
    let output = Command::new(BIN)
        .args(["show", "fish", "-s", "0", "-c", "120"])
        .current_dir(root.join("repo"))
        .env("PATH", path)
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

/// Renders until the background `gh` lookup has landed in the cache.
fn render_with_pr(root: &Path, segments: Value) -> String {
    let deadline = Instant::now() + Duration::from_secs(30);
    loop {
        let prompt = render(root, segments.clone());
        if prompt.contains("#142") {
            return prompt;
        }
        assert!(Instant::now() < deadline, "PR never loaded: {prompt:?}");
        std::thread::sleep(Duration::from_millis(100));
    }
}

#[test]
fn pr_diff_shows_the_prs_line_counts() {
    let root = scratch("diff");

    let pr_only = render_with_pr(&root, json!(["pr"]));
    assert!(!pr_only.contains("+426"), "prompt: {pr_only:?}");

    let both = render_with_pr(&root, json!(["pr", "pr_diff"]));
    assert!(both.contains("+426"), "prompt: {both:?}");
    assert!(both.contains("-35"), "prompt: {both:?}");
    let _ = fs::remove_dir_all(&root);
}

#[test]
fn hovering_over_the_status_dot_lists_the_checks() {
    let root = scratch("hover");

    let prompt = render_with_pr(&root, json!(["pr"]));
    assert!(
        prompt.contains("\x1b]1337;AddHiddenAnnotation=1|2 passed: lint, test\x07"),
        "prompt: {prompt:?}"
    );

    for segment in [
        json!({ "pr": { "hover": false } }),
        json!({ "pr": { "status": false } }),
    ] {
        let prompt = render_with_pr(&root, json!([segment]));
        assert!(!prompt.contains("1337"), "{segment}: {prompt:?}");
    }
    let _ = fs::remove_dir_all(&root);
}
