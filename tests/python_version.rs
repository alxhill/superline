//! The python segment reads the active virtual env's version from
//! `pyvenv.cfg` when it can, and otherwise asks the interpreter through the
//! shared cache. These drive the real binary against fake envs and a seeded
//! cache directory rather than a real interpreter.

use std::fs;
use std::path::PathBuf;
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

use superline::cache::hash_id;

const BIN: &str = env!("CARGO_BIN_EXE_superline");
const PYTHON_ICON: &str = "\u{e73c}";

struct Scratch {
    root: PathBuf,
    cache_dir: PathBuf,
    venv: PathBuf,
    interpreter: PathBuf,
}

/// A throwaway home, cache directory and fake virtual env whose interpreter is
/// an empty file: enough for the widget to locate it, never to run it.
fn scratch(label: &str) -> Scratch {
    let root = std::env::temp_dir().join(format!("superline-pyver-{}-{label}", std::process::id()));
    let _ = fs::remove_dir_all(&root);
    let cache_dir = root.join("cache/superline");
    fs::create_dir_all(&cache_dir).expect("create cache directory");

    let venv = root.join("project/.venv");
    let interpreter = if cfg!(windows) {
        venv.join("Scripts").join("python.exe")
    } else {
        venv.join("bin").join("python")
    };
    fs::create_dir_all(interpreter.parent().unwrap()).expect("create venv bin directory");
    fs::write(&interpreter, b"").expect("write interpreter stub");

    fs::write(
        root.join("config.json"),
        r#"{"theme":"rainbow","rows":[{"left":[{"python":{"version":true}}]}]}"#,
    )
    .expect("write config");

    Scratch {
        root,
        cache_dir,
        venv,
        interpreter,
    }
}

impl Scratch {
    fn cache_file(&self, extension: &str) -> PathBuf {
        self.cache_dir
            .join(format!("python-{}.{extension}", hash_id(&self.interpreter)))
    }

    fn render(&self) -> String {
        let output = Command::new(BIN)
            .args(["show", "fish", "-s", "0", "-c", "120", "--config"])
            .arg(self.root.join("config.json"))
            .current_dir(&self.root)
            .env("HOME", &self.root)
            .env("USERPROFILE", &self.root)
            .env("XDG_CACHE_HOME", self.root.join("cache"))
            .env("LOCALAPPDATA", self.root.join("cache"))
            .env("VIRTUAL_ENV", &self.venv)
            .output()
            .expect("render prompt");
        assert!(output.status.success());
        String::from_utf8_lossy(&output.stdout).into_owned()
    }
}

fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("current time")
        .as_secs()
}

#[test]
fn python_version_is_served_from_the_cache_without_asking_the_interpreter() {
    let scratch = scratch("cached");
    fs::write(
        scratch.cache_file("json"),
        format!(r#"{{"fetched_at":{},"value":"3.12.4"}}"#, now_secs()),
    )
    .expect("seed the version cache");

    let stdout = scratch.render();
    assert!(
        stdout.contains(&format!("{PYTHON_ICON} .venv")),
        "stdout:\n{stdout}"
    );
    assert!(stdout.contains("3.12.4"), "stdout:\n{stdout}");
    // A fresh cache means no refresh is claimed, so the stub is never run.
    assert!(
        !scratch.cache_file("refresh").exists(),
        "a fresh cache should not start a refresh"
    );

    let _ = fs::remove_dir_all(&scratch.root);
}

#[test]
fn python_version_shows_a_loading_marker_until_the_first_refresh_lands() {
    let scratch = scratch("loading");
    // Claim the refresh slot up front so rendering does not try to run the
    // interpreter stub.
    fs::write(
        scratch.cache_file("refresh"),
        (now_secs() as u128 * 1000).to_string(),
    )
    .expect("claim the refresh slot");

    let stdout = scratch.render();
    assert!(
        stdout.contains(&format!("{PYTHON_ICON} .venv")),
        "stdout:\n{stdout}"
    );
    assert!(stdout.contains('\u{2026}'), "stdout:\n{stdout}");

    let _ = fs::remove_dir_all(&scratch.root);
}

#[test]
fn python_version_comes_from_pyvenv_cfg_without_touching_the_cache() {
    let scratch = scratch("pyvenv-cfg");
    fs::write(
        scratch.venv.join("pyvenv.cfg"),
        "home = /nowhere/bin\nimplementation = CPython\nversion_info = 3.99.1\n",
    )
    .expect("write pyvenv.cfg");

    let stdout = scratch.render();
    assert!(stdout.contains("3.99.1"), "stdout:\n{stdout}");
    assert!(!stdout.contains('\u{2026}'), "stdout:\n{stdout}");
    let python_cache_files = fs::read_dir(&scratch.cache_dir)
        .map(|entries| {
            entries
                .flatten()
                .filter(|entry| entry.file_name().to_string_lossy().starts_with("python-"))
                .count()
        })
        .unwrap_or(0);
    assert_eq!(
        python_cache_files, 0,
        "pyvenv.cfg makes the cache unnecessary"
    );

    let _ = fs::remove_dir_all(&scratch.root);
}

#[test]
fn python_version_comes_from_conda_meta_when_there_is_no_pyvenv_cfg() {
    let scratch = scratch("conda-meta");
    let meta = scratch.venv.join("conda-meta");
    fs::create_dir_all(&meta).expect("create conda-meta");
    fs::write(meta.join("python-dateutil-2.9.0-py_0.json"), b"{}").expect("write dateutil meta");
    fs::write(meta.join("python-3.98.2-h1234abc_0.json"), b"{}").expect("write python meta");

    let stdout = scratch.render();
    assert!(stdout.contains("3.98.2"), "stdout:\n{stdout}");
    assert!(!stdout.contains("2.9.0"), "stdout:\n{stdout}");

    let _ = fs::remove_dir_all(&scratch.root);
}
