//! End-to-end checks for the cached Kubernetes widget.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::thread;
use std::time::Duration;

const BIN: &str = env!("CARGO_BIN_EXE_superline");

fn scratch_dir(label: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "superline-kubernetes-{}-{label}",
        std::process::id()
    ));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(dir.join("home/.config/superline")).expect("create config dir");
    fs::write(
        dir.join("home/.config/superline/config.json"),
        r#"{
            "theme": "rainbow",
            "update": { "disable": true },
            "rows": [{ "left": ["kubernetes"] }]
        }"#,
    )
    .expect("write kubernetes config");
    dir
}

fn kubeconfig(current: &str) -> String {
    format!(
        "current-context: {current}\n\
         contexts:\n\
         - name: staging\n  context:\n    namespace: payments\n\
         - name: prod\n  context: {{}}\n"
    )
}

fn render(root: &Path, kubeconfig: Option<&Path>) -> String {
    let home = root.join("home");
    let cache = root.join("cache");
    let mut command = Command::new(BIN);
    command
        .args(["show", "fish", "-s", "0", "-c", "80"])
        .env("HOME", &home)
        .env("USERPROFILE", &home)
        .env("XDG_CACHE_HOME", &cache)
        .env("LOCALAPPDATA", &cache);
    match kubeconfig {
        Some(path) => command.env("KUBECONFIG", path),
        None => command.env_remove("KUBECONFIG"),
    };
    let output = command.output().expect("run superline");
    assert!(
        output.status.success(),
        "show failed:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout).into_owned()
}

#[test]
fn first_prompt_shows_the_context_and_namespace() {
    let root = scratch_dir("cold");
    let config = root.join("kubeconfig");
    fs::write(&config, kubeconfig("staging")).expect("write kubeconfig");

    let prompt = render(&root, Some(&config));
    assert!(prompt.contains("staging (payments)"), "prompt: {prompt}");
    let _ = fs::remove_dir_all(&root);
}

#[test]
fn a_context_switch_shows_on_the_next_prompt() {
    let root = scratch_dir("switch");
    let config = root.join("kubeconfig");
    fs::write(&config, kubeconfig("staging")).expect("write kubeconfig");
    assert!(render(&root, Some(&config)).contains("staging"));

    // Outlast the lookup's one-second TTL, as typing the next command would.
    thread::sleep(Duration::from_millis(1100));
    fs::write(&config, kubeconfig("prod")).expect("switch context");

    let prompt = render(&root, Some(&config));
    assert!(prompt.contains("prod"), "prompt: {prompt}");
    assert!(!prompt.contains("staging"), "prompt: {prompt}");
    let _ = fs::remove_dir_all(&root);
}

#[test]
fn no_kubeconfig_hides_the_segment() {
    let root = scratch_dir("missing");
    let prompt = render(&root, None);
    assert!(!prompt.contains("\u{f10fe}"), "prompt: {prompt}");
    let _ = fs::remove_dir_all(&root);
}
