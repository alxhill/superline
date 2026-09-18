//! End-to-end checks for the display options of the language segments: each
//! can hide the version it would otherwise print, and `java` can also hide the
//! JDK distribution. These drive the real binary against a throwaway `$HOME`
//! holding a config that turns the options off.

use std::fs;
use std::path::PathBuf;
use std::process::Command;

const BIN: &str = env!("CARGO_BIN_EXE_superline");

const MISE_ICON: &str = "\u{f1064}";
const JAVA_ICON: &str = "\u{f0176}";
const NODE_ICON: &str = "\u{ed0d}";
const CARGO_ICON: &str = "\u{e68b}";
const PYTHON_ICON: &str = "\u{e73c}";

fn scratch_dir(label: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "superline-lang-opts-{}-{label}",
        std::process::id()
    ));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).expect("create scratch dir");
    dir
}

/// Render a one-row prompt made of just `segments` from a project containing
/// `files`, with the given mise config declaring the tool versions.
fn render(label: &str, segments: &str, mise_toml: &str, files: &[(&str, &str)]) -> String {
    let root = scratch_dir(label);
    let home = root.join("home");
    let project = root.join("project");
    fs::create_dir_all(home.join(".config/superline")).expect("create scratch config dir");
    fs::create_dir_all(&project).expect("create project dir");

    fs::write(
        home.join(".config/superline/config.json"),
        format!(r#"{{ "theme": "rainbow", "rows": [ {{ "left": [ {segments} ] }} ] }}"#),
    )
    .expect("write config");

    fs::write(project.join("mise.toml"), mise_toml).expect("write mise config");
    for (name, contents) in files {
        fs::write(project.join(name), contents).expect("write project file");
    }

    let output = Command::new(BIN)
        .args(["show", "fish", "-s", "0", "-c", "200"])
        .current_dir(&project)
        .env("HOME", &home)
        .env("USERPROFILE", &home)
        .env_remove("SDKMAN_ENV")
        .env_remove("nvm_current_version")
        .env_remove("VIRTUAL_ENV")
        .env_remove("CONDA_ENV_PATH")
        .env_remove("CONDA_DEFAULT_ENV")
        .output()
        .expect("failed to run the superline binary");

    assert!(
        output.status.success(),
        "`show fish` exited with failure\nstderr:\n{}",
        String::from_utf8_lossy(&output.stderr),
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        !stderr.contains("could not be parsed"),
        "config with language options failed to parse\nstderr:\n{stderr}",
    );

    let _ = fs::remove_dir_all(&root);
    String::from_utf8_lossy(&output.stdout).into_owned()
}

fn assert_shown(prompt: &str, needle: &str, what: &str) {
    assert!(
        prompt.contains(needle),
        "expected {what} in the prompt:\n{prompt}"
    );
}

fn assert_hidden(prompt: &str, needle: &str, what: &str) {
    assert!(
        !prompt.contains(needle),
        "expected {what} to be hidden from the prompt:\n{prompt}"
    );
}

const JAVA_MISE: &str = "[tools]\njava = \"temurin-21.0.5\"\n";

#[test]
fn java_shows_both_version_and_jdk_by_default() {
    let prompt = render("java-default", r#""java""#, JAVA_MISE, &[]);

    assert_shown(&prompt, "21 Temurin", "the java version and distribution");
    assert_shown(&prompt, MISE_ICON, "the mise marker");
}

#[test]
fn java_jdk_can_be_hidden() {
    let prompt = render(
        "java-no-jdk",
        r#"{ "java": { "jdk": false } }"#,
        JAVA_MISE,
        &[],
    );

    assert_shown(&prompt, &format!("{JAVA_ICON} 21"), "the java version");
    assert_hidden(&prompt, "Temurin", "the JDK distribution");
}

#[test]
fn java_version_can_be_hidden() {
    let prompt = render(
        "java-no-version",
        r#"{ "java": { "version": false } }"#,
        JAVA_MISE,
        &[],
    );

    assert_shown(
        &prompt,
        &format!("{JAVA_ICON} Temurin"),
        "the JDK distribution",
    );
    assert_hidden(&prompt, "21", "the java version");
}

#[test]
fn java_can_be_reduced_to_its_icon() {
    let prompt = render(
        "java-icon-only",
        r#"{ "java": { "version": false, "jdk": false } }"#,
        JAVA_MISE,
        &[],
    );

    assert_shown(
        &prompt,
        &format!("{MISE_ICON} {JAVA_ICON}"),
        "the mise and java icons",
    );
    assert_hidden(&prompt, "21", "the java version");
    assert_hidden(&prompt, "Temurin", "the JDK distribution");
}

#[test]
fn node_version_can_be_hidden() {
    let mise = "[tools]\nnode = \"22.14.0\"\n";

    let shown = render("node-default", r#""nvm""#, mise, &[]);
    assert_shown(&shown, "22.14.0", "the node version");

    let hidden = render(
        "node-no-version",
        r#"{ "nvm": { "version": false } }"#,
        mise,
        &[],
    );
    assert_shown(
        &hidden,
        &format!("{MISE_ICON} {NODE_ICON}"),
        "the node icon",
    );
    assert_hidden(&hidden, "22.14.0", "the node version");
}

#[test]
fn rust_toolchain_version_can_be_hidden() {
    let mise = "[tools]\nrust = \"1.93.0\"\n";
    let files = [("Cargo.toml", "[package]\nname = \"scratch\"\n")];

    let shown = render("rust-default", r#""cargo""#, mise, &files);
    assert_shown(&shown, "1.93.0", "the toolchain version");

    let hidden = render(
        "rust-no-version",
        r#"{ "cargo": { "version": false } }"#,
        mise,
        &files,
    );
    assert_shown(
        &hidden,
        &format!("{MISE_ICON} {CARGO_ICON}"),
        "the cargo icon",
    );
    assert_hidden(&hidden, "1.93.0", "the toolchain version");
}

#[test]
fn rust_toolchain_file_pins_the_cargo_version() {
    let files = [
        ("Cargo.toml", "[package]\nname = \"scratch\"\n"),
        (
            "rust-toolchain.toml",
            "[toolchain]\nchannel = \"1.85.0\"\ncomponents = [\"clippy\"]\n",
        ),
    ];

    let prompt = render("rust-toolchain-toml", r#""cargo""#, "", &files);
    assert_shown(
        &prompt,
        &format!("{CARGO_ICON} 1.85.0"),
        "the toolchain channel",
    );
    assert_hidden(&prompt, MISE_ICON, "the mise marker");

    let hidden = render(
        "rust-toolchain-no-version",
        r#"{ "cargo": { "version": false } }"#,
        "",
        &files,
    );
    assert_hidden(&hidden, "1.85.0", "the toolchain channel");
}

#[test]
fn legacy_rust_toolchain_file_pins_the_cargo_version() {
    let prompt = render(
        "rust-toolchain-legacy",
        r#""cargo""#,
        "",
        &[
            ("Cargo.toml", "[package]\nname = \"scratch\"\n"),
            ("rust-toolchain", "nightly-2025-01-15\n"),
        ],
    );

    assert_shown(
        &prompt,
        &format!("{CARGO_ICON} nightly-2025-01-15"),
        "the legacy toolchain channel",
    );
}

#[test]
fn mise_rust_wins_over_a_rust_toolchain_file() {
    let prompt = render(
        "rust-both",
        r#""cargo""#,
        "[tools]\nrust = \"1.93.0\"\n",
        &[
            ("Cargo.toml", "[package]\nname = \"scratch\"\n"),
            ("rust-toolchain.toml", "[toolchain]\nchannel = \"1.85.0\"\n"),
        ],
    );

    assert_shown(&prompt, "1.93.0", "the mise toolchain version");
    assert_hidden(&prompt, "1.85.0", "the rust-toolchain channel");
}

#[test]
fn python_version_is_on_by_default_and_can_be_hidden() {
    let mise = "[tools]\npython = \"3.13.3\"\n";

    let shown = render("python-default", r#""python_env""#, mise, &[]);
    assert_shown(
        &shown,
        &format!("{MISE_ICON} {PYTHON_ICON} 3.13.3"),
        "the python version in the same segment as the icon",
    );

    let hidden = render(
        "python-no-version",
        r#"{ "python_env": { "version": false } }"#,
        mise,
        &[],
    );
    assert_shown(
        &hidden,
        &format!("{MISE_ICON} {PYTHON_ICON}"),
        "the mise marker and python icon",
    );
    assert_hidden(&hidden, "3.13.3", "the python version");
}
