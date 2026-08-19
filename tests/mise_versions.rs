//! End-to-end checks that the language segments pick up versions declared in a
//! [mise](https://mise.jdx.dev) config, and mark them as mise-managed.
//!
//! These drive the real binary with its working directory inside a throwaway
//! project so the config discovery walk is exercised for real.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

const BIN: &str = env!("CARGO_BIN_EXE_superline");

/// The marker segments print next to a version a mise config is responsible for.
const MISE_ICON: &str = "\u{f1064}";

fn scratch_dir(label: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("superline-mise-{}-{label}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).expect("create scratch dir");
    dir
}

/// Render the default prompt from `cwd`, with `$HOME` pointed at a scratch dir
/// so no real user config or global mise config takes part. The language
/// segments sit on the right of the last row, which the shell renders through
/// `show-right`, so both halves are captured.
fn render_from(cwd: &Path, home: &Path) -> String {
    [run(cwd, home, "show"), run(cwd, home, "show-right")].join("\n")
}

fn run(cwd: &Path, home: &Path, subcommand: &str) -> String {
    let output = Command::new(BIN)
        .args([subcommand, "fish", "-s", "0", "-c", "200"])
        .current_dir(cwd)
        .env("HOME", home)
        .env("USERPROFILE", home)
        // sdkman's auto-env exports this; clear it so only mise is in play.
        .env_remove("SDKMAN_ENV")
        .output()
        .expect("failed to run the superline binary");

    assert!(
        output.status.success(),
        "`{subcommand} fish` exited with failure\nstderr:\n{}",
        String::from_utf8_lossy(&output.stderr),
    );

    String::from_utf8_lossy(&output.stdout).into_owned()
}

/// Set up a project directory and a warmed scratch `$HOME`, then render in it.
fn render_project(label: &str, files: &[(&str, &str)]) -> String {
    let root = scratch_dir(label);
    let home = root.join("home");
    let project = root.join("project");
    fs::create_dir_all(&home).expect("create scratch home");
    fs::create_dir_all(&project).expect("create project dir");

    for (name, contents) in files {
        fs::write(project.join(name), contents).expect("write project file");
    }

    // Warm the default config so the one-time "creating config" notice is gone.
    let _ = render_from(&project, &home);
    let out = render_from(&project, &home);

    let _ = fs::remove_dir_all(&root);
    out
}

#[test]
fn java_version_comes_from_a_mise_config() {
    let prompt = render_project(
        "java",
        &[("mise.toml", "[tools]\njava = \"corretto-26.0.2.10.1\"\n")],
    );

    assert!(
        prompt.contains("26 corretto"),
        "expected the mise java version in the prompt:\n{prompt}",
    );
    assert!(
        prompt.contains(MISE_ICON),
        "expected the mise marker next to the java version:\n{prompt}",
    );
}

#[test]
fn mise_java_wins_over_a_sdkmanrc_in_the_same_repo() {
    // Mirrors a repo that keeps `.sdkmanrc` around for contributors who have not
    // moved to mise: mise is what actually puts a JDK on the path.
    let prompt = render_project(
        "java-both",
        &[
            ("mise.toml", "[tools]\njava = \"temurin-21.0.5\"\n"),
            (".sdkmanrc", "java=17.0.9-amzn\n"),
        ],
    );

    assert!(
        prompt.contains("21 Temurin"),
        "expected the mise java version to win:\n{prompt}",
    );
    assert!(
        !prompt.contains("17 corretto"),
        "the .sdkmanrc version should not be shown:\n{prompt}",
    );
}

#[test]
fn other_mise_managed_languages_are_shown() {
    let prompt = render_project(
        "languages",
        &[
            (
                ".mise.toml",
                "[tools]\nnode = \"22.14.0\"\npython = \"3.13.3\"\nrust = \"1.93.0\"\n",
            ),
            ("Cargo.toml", "[package]\nname = \"scratch\"\n"),
        ],
    );

    for version in ["22.14.0", "3.13.3", "1.93.0"] {
        assert!(
            prompt.contains(version),
            "expected the mise {version} in the prompt:\n{prompt}",
        );
    }
}

#[test]
fn a_repo_without_mise_is_unchanged() {
    let prompt = render_project("none", &[("Cargo.toml", "[package]\nname = \"scratch\"\n")]);

    assert!(
        !prompt.contains(MISE_ICON),
        "the mise marker should not appear without a mise config:\n{prompt}",
    );
}
