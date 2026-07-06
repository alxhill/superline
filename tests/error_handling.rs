use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

const BIN: &str = env!("CARGO_BIN_EXE_superline");

fn scratch_dir(label: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("superline-errors-{}-{label}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).expect("create scratch dir");
    dir
}

fn run_show(config: &Path) -> Output {
    Command::new(BIN)
        .args(["show", "fish", "-s", "0", "-c", "80", "--config"])
        .arg(config)
        .output()
        .expect("failed to run superline")
}

#[test]
fn future_module_names_render_as_unknown_modules() {
    let dir = scratch_dir("future-module");
    let config = dir.join("config.json");
    fs::write(
        &config,
        r#"{
            "theme": "simple",
            "rows": [
                {
                    "left": ["future_module"],
                    "right": []
                }
            ]
        }"#,
    )
    .expect("write config");

    let output = run_show(&config);
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    let _ = fs::remove_dir_all(&dir);

    assert!(
        output.status.success(),
        "show exited with failure: {stderr}"
    );
    assert!(
        stdout.contains("unknown module: future_module"),
        "future module should render visibly\nstdout:\n{stdout}\nstderr:\n{stderr}",
    );
    assert!(
        !stderr.contains("config file could not be parsed"),
        "future module should not invalidate the whole config\nstderr:\n{stderr}",
    );
}

#[test]
fn invalid_config_renders_fallback_with_error() {
    let dir = scratch_dir("bad-config");
    let config = dir.join("config.json");
    fs::write(&config, "{ not json").expect("write invalid config");

    let output = run_show(&config);
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    let _ = fs::remove_dir_all(&dir);

    assert!(
        output.status.success(),
        "show exited with failure: {stderr}"
    );
    assert!(
        stdout.contains("config file not parsed"),
        "fallback should include short config parse error\nstdout:\n{stdout}\nstderr:\n{stderr}",
    );
    assert!(
        !stdout.contains("expected `:`"),
        "fallback should not include low-level parser details\nstdout:\n{stdout}",
    );
    assert!(
        stdout.contains("superline"),
        "fallback should still render the default cwd prompt\nstdout:\n{stdout}\nstderr:\n{stderr}",
    );
    assert!(
        !stderr.contains("superline error"),
        "fallback should not also print a stderr error\nstderr:\n{stderr}",
    );
}

#[test]
fn invalid_custom_theme_renders_fallback_with_error() {
    let dir = scratch_dir("bad-theme");
    let config = dir.join("config.json");
    let theme = dir.join("bad_theme.json");

    fs::write(
        &config,
        r#"{
            "theme": "bad_theme.json",
            "rows": [
                {
                    "left": ["future_module", "cmd"],
                    "right": []
                }
            ]
        }"#,
    )
    .expect("write config");
    fs::write(
        &theme,
        r#"{
            "defaults": {
                "fg": "not_a_color",
                "bg": "black"
            },
            "modules": {}
        }"#,
    )
    .expect("write theme");

    let output = run_show(&config);
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    let _ = fs::remove_dir_all(&dir);

    assert!(
        output.status.success(),
        "show exited with failure: {stderr}"
    );
    assert!(
        stdout.contains("theme file invalid"),
        "fallback should include short theme validation error\nstdout:\n{stdout}\nstderr:\n{stderr}",
    );
    assert!(
        !stdout.contains("not_a_color"),
        "fallback should not include low-level theme details\nstdout:\n{stdout}",
    );
    assert!(
        stdout.contains("unknown module: future_module"),
        "theme fallback should keep the configured prompt modules\nstdout:\n{stdout}",
    );
    assert!(
        !stderr.contains("superline error"),
        "fallback should not also print a stderr error\nstderr:\n{stderr}",
    );
}
