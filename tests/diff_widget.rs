//! End-to-end checks for the diff widget's working-tree counts in a shell
//! prompt.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, Instant};

use serde_json::json;

const BIN: &str = env!("CARGO_BIN_EXE_superline");

/// A repository at `<root>/repo` with one committed five-line file, and an
/// empty scratch home.
fn scratch(label: &str) -> PathBuf {
    let root = std::env::temp_dir().join(format!(
        "superline-diff-widget-{}-{label}",
        std::process::id()
    ));
    let _ = fs::remove_dir_all(&root);
    let config_dir = root.join("home/.config/superline");
    fs::create_dir_all(&config_dir).expect("create config dir");
    let config = json!({
        "theme": "rainbow",
        "update": { "disable": true },
        "rows": [{ "left": ["diff"] }],
    });
    fs::write(config_dir.join("config.json"), config.to_string()).expect("write config");

    let repo = root.join("repo");
    fs::create_dir_all(&repo).expect("create repo dir");
    git(&repo, &["init", "-q", "-b", "main"]);
    git(&repo, &["config", "user.email", "test@example.com"]);
    git(&repo, &["config", "user.name", "test"]);
    fs::write(repo.join("notes.txt"), "one\ntwo\nthree\nfour\nfive\n").expect("write file");
    git(&repo, &["add", "notes.txt"]);
    git(&repo, &["commit", "-q", "-m", "init"]);
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

fn render(root: &Path) -> String {
    let home = root.join("home");
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

/// Renders until the prompt contains `expected`, giving a cold diff lookup
/// time to land in the cache.
fn render_until(root: &Path, expected: &str) -> String {
    let deadline = Instant::now() + Duration::from_secs(30);
    loop {
        let prompt = render(root);
        if prompt.contains(expected) {
            return prompt;
        }
        assert!(
            Instant::now() < deadline,
            "never saw {expected:?}: {prompt:?}"
        );
        std::thread::sleep(Duration::from_millis(100));
    }
}

#[test]
fn a_clean_tree_shows_nothing_and_edits_show_their_line_counts() {
    let root = scratch("counts");
    let repo = root.join("repo");

    // Let the clean tree's lookup land before checking it is empty.
    let _ = render(&root);
    std::thread::sleep(Duration::from_secs(1));
    let clean = render(&root);
    assert!(!clean.contains('+'), "prompt: {clean:?}");

    // Two lines replaced and one appended, the latter staged separately.
    fs::write(repo.join("notes.txt"), "one\n2\n3\nfour\nfive\nsix\n").expect("edit file");
    git(&repo, &["add", "notes.txt"]);
    fs::write(repo.join("other.txt"), "untracked\n").expect("write untracked");
    let prompt = render_until(&root, "+3");
    assert!(prompt.contains("-2"), "prompt: {prompt:?}");
    let _ = fs::remove_dir_all(&root);
}
