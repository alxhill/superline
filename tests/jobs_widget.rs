//! End-to-end checks for the shell-supplied Jobs widget.

use std::fs;
use std::path::PathBuf;
use std::process::Command;

const BIN: &str = env!("CARGO_BIN_EXE_superline");
const JOBS_SYMBOL: &str = "\u{f085}";

fn scratch_dir(label: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("superline-jobs-{}-{label}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).expect("create scratch dir");
    dir
}

fn render(jobs: usize) -> String {
    let root = scratch_dir(&format!("render-{jobs}"));
    let home = root.join("home");
    let cache = root.join("cache");
    let config_dir = home.join(".config/superline");
    fs::create_dir_all(&config_dir).expect("create config dir");
    fs::write(
        config_dir.join("config.json"),
        r#"{
            "theme": "rainbow",
            "update": { "disable": true },
            "rows": [{ "left": ["jobs"] }]
        }"#,
    )
    .expect("write jobs config");

    let output = Command::new(BIN)
        .args([
            "show",
            "fish",
            "-s",
            "0",
            "-c",
            "80",
            "--jobs",
            &jobs.to_string(),
        ])
        .env("HOME", &home)
        .env("USERPROFILE", &home)
        .env("XDG_CACHE_HOME", &cache)
        .output()
        .expect("run superline");
    assert!(
        output.status.success(),
        "show failed:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );

    let prompt = String::from_utf8_lossy(&output.stdout).into_owned();
    let _ = fs::remove_dir_all(&root);
    prompt
}

#[test]
fn jobs_are_hidden_when_the_shell_reports_none() {
    assert!(!render(0).contains(JOBS_SYMBOL));
}

#[test]
fn one_job_shows_the_symbol_and_count() {
    let prompt = render(1);
    assert!(prompt.contains("\u{f085} 1"), "prompt was: {prompt}");
}

#[test]
fn multiple_jobs_show_the_symbol_and_count() {
    assert!(render(3).contains("\u{f085} 3"));
}

#[test]
fn every_supported_shell_passes_a_job_count() {
    for shell in ["bash", "zsh", "fish", "pwsh", "nu"] {
        let output = Command::new(BIN)
            .args(["init", shell])
            .output()
            .unwrap_or_else(|_| panic!("run init {shell}"));
        assert!(output.status.success(), "init {shell} failed");
        let init = String::from_utf8_lossy(&output.stdout);
        assert!(
            init.contains("--jobs"),
            "init {shell} does not pass a job count:\n{init}"
        );
    }
}

#[test]
fn every_shell_captures_status_before_counting_jobs() {
    let cases = [
        ("bash", "local __pl_status=$?", "local __pl_jobs="),
        ("zsh", "__pl_status=$?", "__pl_running=("),
        ("fish", "set -l __pl_status $status", "set -l __pl_jobs"),
        ("pwsh", "$__pl_ok = $?", "$__pl_jobs = @(Get-Job"),
        (
            "nu",
            "let __pl_status = ($env.LAST_EXIT_CODE? | default 0)",
            "job list | where",
        ),
    ];

    for (shell, status_marker, jobs_marker) in cases {
        let output = Command::new(BIN)
            .args(["init", shell])
            .output()
            .unwrap_or_else(|_| panic!("run init {shell}"));
        assert!(output.status.success(), "init {shell} failed");
        let init = String::from_utf8_lossy(&output.stdout);
        let status_offset = init
            .find(status_marker)
            .unwrap_or_else(|| panic!("init {shell} has no status capture:\n{init}"));
        let jobs_offset = init
            .find(jobs_marker)
            .unwrap_or_else(|| panic!("init {shell} has no jobs lookup:\n{init}"));
        assert!(
            status_offset < jobs_offset,
            "init {shell} looks up jobs before preserving status:\n{init}"
        );
    }
}

/// Stopped jobs linger in the job table long after the user has forgotten
/// them - fish and PowerShell keep them indefinitely - so every initializer
/// has to count running jobs only, or the widget never leaves the prompt.
#[test]
fn every_shell_counts_running_jobs_only() {
    let cases = [
        (
            "bash",
            "$(jobs -pr 2>/dev/null | wc -l)",
            "$(jobs -p 2>/dev/null | wc -l)",
        ),
        (
            "zsh",
            "${(M)${(@v)jobstates}:#running*}",
            "${#jobstates[*]}",
        ),
        (
            "fish",
            r"string match -r --entire '\trunning\t'",
            "jobs -g 2>/dev/null | count",
        ),
        (
            "pwsh",
            "@(Get-Job | Where-Object { $_.State -eq 'Running' }).Count",
            "@(Get-Job).Count",
        ),
        (
            "nu",
            r#"where {|j| $j.type? != "frozen" }"#,
            "job list | length",
        ),
    ];

    for (shell, running_only, counts_everything) in cases {
        let output = Command::new(BIN)
            .args(["init", shell])
            .output()
            .unwrap_or_else(|_| panic!("run init {shell}"));
        assert!(output.status.success(), "init {shell} failed");
        let init = String::from_utf8_lossy(&output.stdout);
        assert!(
            init.contains(running_only),
            "init {shell} does not filter out stopped jobs:\n{init}"
        );
        assert!(
            !init.contains(counts_everything),
            "init {shell} still counts every job:\n{init}"
        );
    }
}
