//! End-to-end checks for `git_remote`, the git widget's remote segment as a
//! widget of its own: configs that only name `git` draw what they always did,
//! the split pair draws the same prompt, and the two share one status lookup.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use serde_json::{json, Value};

const BIN: &str = env!("CARGO_BIN_EXE_superline");
const REPO_URL: &str = "https://github.com/example/repo";

/// A repository at `<root>/repo` pushed to a bare remote, then one commit
/// ahead of it with a modified, a staged and an untracked file. `origin` is
/// pointed at a GitHub URL afterwards so the logo has a page to link to while
/// the remote-tracking ref still gives the ahead count.
fn scratch(label: &str) -> PathBuf {
    let root = std::env::temp_dir().join(format!(
        "superline-git-remote-{}-{label}",
        std::process::id()
    ));
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(root.join("home/.config/superline")).expect("create config dir");

    let (repo, remote) = (root.join("repo"), root.join("remote.git"));
    fs::create_dir_all(&repo).expect("create repo dir");
    git(&root, &["init", "-q", "--bare", "remote.git"]);
    git(&repo, &["init", "-q", "-b", "main"]);
    git(&repo, &["config", "user.email", "test@example.com"]);
    git(&repo, &["config", "user.name", "test"]);
    fs::write(repo.join("README.md"), "# demo\n").expect("write file");
    git(&repo, &["add", "README.md"]);
    git(&repo, &["commit", "-q", "-m", "init"]);
    git(
        &repo,
        &["remote", "add", "origin", remote.to_str().unwrap()],
    );
    // A global `push.negotiate` only adds noise against a local bare remote.
    git(
        &repo,
        &[
            "-c",
            "push.negotiate=false",
            "push",
            "-q",
            "-u",
            "origin",
            "main",
        ],
    );
    git(&repo, &["commit", "-q", "--allow-empty", "-m", "local"]);
    git(
        &repo,
        &["remote", "set-url", "origin", &format!("{REPO_URL}.git")],
    );

    fs::write(repo.join("README.md"), "# demo\nchanged\n").expect("modify file");
    fs::write(repo.join("staged.rs"), "fn main() {}\n").expect("write file");
    git(&repo, &["add", "staged.rs"]);
    fs::write(repo.join("notes.md"), "").expect("write file");
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

/// Runs the prompt for a row of `widgets` from the repository, with
/// `SUPERLINE_DEBUG` set when `debug` is.
fn run(root: &Path, widgets: Value, debug: bool) -> Output {
    let home = root.join("home");
    let config = json!({
        "theme": "rainbow",
        "update": { "disable": true },
        "rows": [{ "left": widgets }],
    });
    fs::write(
        home.join(".config/superline/config.json"),
        config.to_string(),
    )
    .expect("write config");

    let cache = root.join("cache");
    let mut command = Command::new(BIN);
    command
        .args(["show", "fish", "-s", "0", "-c", "120"])
        .current_dir(root.join("repo"))
        .env("HOME", &home)
        .env("USERPROFILE", &home)
        .env("XDG_CACHE_HOME", &cache)
        .env("LOCALAPPDATA", &cache)
        .env_remove("SUPERLINE_DEBUG");
    if debug {
        command.env("SUPERLINE_DEBUG", "1");
    }
    let output = command.output().expect("run superline");
    assert!(
        output.status.success(),
        "show failed:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
    output
}

fn render(root: &Path, widgets: Value) -> String {
    String::from_utf8_lossy(&run(root, widgets, false).stdout).into_owned()
}

/// A git widget that waits long enough for a cold status on a loaded
/// machine, with `options` on top.
fn git_widget(backend: &str, options: Value) -> Value {
    let mut git = json!({ "status_timeout_ms": 30_000, "backend": backend });
    git.as_object_mut()
        .unwrap()
        .extend(options.as_object().unwrap().clone());
    json!({ "git": git })
}

#[test]
fn the_split_pair_draws_what_git_alone_always_did() {
    let root = scratch("same");
    for backend in ["cli", "gitoxide"] {
        let combined = render(&root, json!([git_widget(backend, json!({}))]));
        // The remote segment with its link and the ahead count is there.
        assert!(combined.contains(REPO_URL), "{backend}: {combined:?}");
        assert!(combined.contains("1\u{f062}"), "{backend}: {combined:?}");

        let split = render(
            &root,
            json!([git_widget(backend, json!({ "repo": false })), "git_remote"]),
        );
        assert_eq!(split, combined, "{backend}");
    }
    let _ = fs::remove_dir_all(&root);
}

#[test]
fn the_remote_segment_can_move_away_from_the_branch() {
    let root = scratch("moved");
    let prompt = render(
        &root,
        json!(["git_remote", git_widget("auto", json!({ "repo": false }))]),
    );
    let (link, branch) = (prompt.find(REPO_URL), prompt.find("main"));
    assert!(
        matches!((link, branch), (Some(link), Some(branch)) if link < branch),
        "prompt: {prompt:?}"
    );
    // It is drawn once, by `git_remote`.
    assert_eq!(prompt.matches(REPO_URL).count(), 1, "prompt: {prompt:?}");
    let _ = fs::remove_dir_all(&root);
}

#[test]
fn ahead_behind_off_leaves_just_the_link() {
    let root = scratch("link-only");
    let prompt = render(
        &root,
        json!([
            git_widget("auto", json!({ "repo": false })),
            { "git_remote": { "ahead_behind": false } }
        ]),
    );
    assert!(prompt.contains(REPO_URL), "prompt: {prompt:?}");
    assert!(!prompt.contains("\u{f062}"), "prompt: {prompt:?}");
    let _ = fs::remove_dir_all(&root);
}

/// The debug report lists each cached lookup a widget made. `git_remote`
/// reuses the status `git` looked up instead of walking the tree again.
#[test]
fn the_pair_looks_the_status_up_once() {
    let root = scratch("shared");
    let output = run(
        &root,
        json!([git_widget("auto", json!({ "repo": false })), "git_remote"]),
        true,
    );
    let report = String::from_utf8_lossy(&output.stderr);
    let lookups: Vec<&str> = report
        .lines()
        .map(str::trim_start)
        .filter(|line| line.starts_with("git "))
        .collect();
    assert_eq!(lookups.len(), 2, "report:\n{report}");
    assert!(
        !lookups[0].contains("shared"),
        "the first lookup is git's own:\n{report}"
    );
    assert!(
        lookups[1].contains("shared with an earlier widget"),
        "report:\n{report}"
    );
    let _ = fs::remove_dir_all(&root);
}
