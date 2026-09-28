//! End-to-end checks for the linked-worktree count in the git widget.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use serde_json::{json, Value};

const BIN: &str = env!("CARGO_BIN_EXE_superline");
const WORKTREE_ICON: &str = "\u{f1897}";

/// A repository at `<root>/main` with linked worktrees at `<root>/one` and
/// `<root>/two`, and an empty scratch home.
fn scratch(label: &str) -> PathBuf {
    let root = std::env::temp_dir().join(format!(
        "superline-git-worktrees-{}-{label}",
        std::process::id()
    ));
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(root.join("home/.config/superline")).expect("create config dir");

    let main = root.join("main");
    fs::create_dir_all(&main).expect("create repo dir");
    git(&main, &["init", "-q", "-b", "main"]);
    git(&main, &["config", "user.email", "test@example.com"]);
    git(&main, &["config", "user.name", "test"]);
    git(&main, &["commit", "-q", "--allow-empty", "-m", "init"]);
    for branch in ["one", "two"] {
        let path = root.join(branch);
        git(
            &main,
            &[
                "worktree",
                "add",
                "-q",
                "-b",
                branch,
                path.to_str().unwrap(),
            ],
        );
    }
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

/// Renders a prompt holding only `segment` from `cwd`, themed by `theme`
/// (a built-in name, or a theme file's contents).
fn render(root: &Path, cwd: &str, segment: Value, theme: Value) -> String {
    let home = root.join("home");
    let config_dir = home.join(".config/superline");
    let theme = match theme {
        Value::String(name) => name,
        file => {
            fs::write(config_dir.join("theme.json"), file.to_string()).expect("write theme");
            String::from("theme.json")
        }
    };
    let config = json!({
        "theme": theme,
        "update": { "disable": true },
        "rows": [{ "left": [segment] }],
    });
    fs::write(config_dir.join("config.json"), config.to_string()).expect("write config");

    let cache = root.join("cache");
    let output = Command::new(BIN)
        .args(["show", "fish", "-s", "0", "-c", "120"])
        .current_dir(root.join(cwd))
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

/// A git segment that waits long enough for a cold status on a loaded machine.
fn git_segment(options: Value) -> Value {
    let mut options = options;
    options["status_timeout_ms"] = json!(30_000);
    json!({ "git": options })
}

#[test]
fn the_count_follows_the_branch_from_every_checkout_with_either_backend() {
    let root = scratch("count");
    for backend in ["cli", "gitoxide"] {
        for (cwd, branch) in [("main", "main"), ("one", "one"), ("two", "two")] {
            let prompt = render(
                &root,
                cwd,
                git_segment(json!({ "backend": backend })),
                json!("rainbow"),
            );
            let expected = format!("{branch} {WORKTREE_ICON} 2 ");
            assert!(
                prompt.contains(&expected),
                "{backend} from {cwd}: {prompt:?}"
            );
        }
    }
    let _ = fs::remove_dir_all(&root);
}

#[test]
fn turning_worktrees_off_hides_the_count() {
    let root = scratch("off");
    let prompt = render(
        &root,
        "main",
        git_segment(json!({ "worktrees": false })),
        json!("rainbow"),
    );
    assert!(prompt.contains("main "), "prompt: {prompt:?}");
    assert!(!prompt.contains(WORKTREE_ICON), "prompt: {prompt:?}");
    assert!(!prompt.contains("main 2"), "prompt: {prompt:?}");
    let _ = fs::remove_dir_all(&root);
}

#[test]
fn a_theme_can_swap_or_hide_the_icon() {
    let root = scratch("icon");
    let theme = |icon: &str| {
        json!({
            "defaults": { "fg": 15, "bg": 0 },
            "modules": { "git": { "worktree_icon": icon } },
        })
    };

    let prompt = render(&root, "main", git_segment(json!({})), theme("wt"));
    assert!(prompt.contains("main wt 2 "), "prompt: {prompt:?}");

    let prompt = render(&root, "main", git_segment(json!({})), theme(""));
    assert!(prompt.contains("main 2 "), "prompt: {prompt:?}");
    assert!(!prompt.contains(WORKTREE_ICON), "prompt: {prompt:?}");
    let _ = fs::remove_dir_all(&root);
}
