//! End-to-end checks for the PR widget, with `gh` answered by the site
//! screenshots' stub.

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

fn render(root: &Path, segment: Value) -> String {
    let home = root.join("home");
    let config = json!({
        "theme": "rainbow",
        "update": { "disable": true },
        "rows": [{ "left": [segment] }],
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
fn render_with_pr(root: &Path, segment: Value) -> String {
    let deadline = Instant::now() + Duration::from_secs(30);
    loop {
        let prompt = render(root, segment.clone());
        if prompt.contains("#142") {
            return prompt;
        }
        assert!(Instant::now() < deadline, "PR never loaded: {prompt:?}");
        std::thread::sleep(Duration::from_millis(100));
    }
}

#[cfg(unix)]
#[test]
fn diff_shows_the_prs_line_counts_only_when_enabled() {
    let root = scratch("diff");

    let shown = render_with_pr(&root, json!({ "pr": { "diff": true } }));
    assert!(shown.contains("+426"), "prompt: {shown:?}");
    assert!(shown.contains("-35"), "prompt: {shown:?}");

    let hidden = render_with_pr(&root, json!("pr"));
    assert!(!hidden.contains("+426"), "prompt: {hidden:?}");
    assert!(!hidden.contains("-35"), "prompt: {hidden:?}");
    let _ = fs::remove_dir_all(&root);
}
