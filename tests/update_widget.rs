//! Drives the compiled binary against a pre-seeded update cache to check the
//! notice renders once, links to the release, and then stays hidden.

use std::fs;
use std::path::PathBuf;
use std::process::{Command, Output};
use std::time::{SystemTime, UNIX_EPOCH};

const BIN: &str = env!("CARGO_BIN_EXE_superline");
const CURRENT_VERSION: &str = env!("CARGO_PKG_VERSION");

struct Fixture {
    root: PathBuf,
    cache_dir: PathBuf,
    config: PathBuf,
}

impl Fixture {
    fn new(label: &str, command: &str) -> Self {
        let root =
            std::env::temp_dir().join(format!("superline-update-{label}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        let cache_dir = root.join("cache/superline");
        fs::create_dir_all(&cache_dir).expect("create cache directory");

        let config = root.join("config.json");
        fs::write(
            &config,
            format!(
                r#"{{"theme":"rainbow","rows":[{{"left":[{{"update":{{"command":"{command}"}}}}]}}]}}"#
            ),
        )
        .expect("write config");
        Fixture {
            root,
            cache_dir,
            config,
        }
    }

    /// Seeds the cache as a finished lookup would have, so rendering never
    /// starts a real check.
    fn cache_release(&self, version: &str) {
        let fetched_at = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("current time")
            .as_secs();
        fs::write(
            self.cache_dir.join("update-latest.json"),
            format!(
                r#"{{"fetched_at":{fetched_at},"value":{{"version":"{version}","url":"https://github.com/alxhill/superline/releases/tag/{version}"}}}}"#
            ),
        )
        .expect("write update cache");
    }

    fn shown_marker(&self) -> PathBuf {
        self.cache_dir.join("update-latest.shown")
    }

    fn render(&self) -> String {
        let output: Output = Command::new(BIN)
            .args(["show", "fish", "-s", "0", "-c", "160", "--config"])
            .arg(&self.config)
            .env("HOME", &self.root)
            .env("USERPROFILE", &self.root)
            .env("XDG_CACHE_HOME", self.root.join("cache"))
            .env("LOCALAPPDATA", self.root.join("cache"))
            .output()
            .expect("render prompt");
        assert!(output.status.success());
        String::from_utf8_lossy(&output.stdout).into_owned()
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

#[test]
fn a_newer_release_is_announced_once_with_the_upgrade_command() {
    let fixture = Fixture::new("newer", "brew upgrade superline");
    fixture.cache_release("v99.0.0");

    let first = fixture.render();
    assert!(
        first.contains("superline v99.0.0 available: brew upgrade superline"),
        "stdout:\n{first}"
    );
    assert!(
        first.contains("https://github.com/alxhill/superline/releases/tag/v99.0.0"),
        "the notice should link to the release\nstdout:\n{first}"
    );
    assert!(fixture.shown_marker().is_file());

    // The same day, the notice stays hidden even though the release is still
    // newer than this binary.
    let second = fixture.render();
    assert!(!second.contains("v99.0.0"), "stdout:\n{second}");
}

#[test]
fn a_notice_shown_yesterday_is_shown_again() {
    let fixture = Fixture::new("yesterday", "cargo binstall superline");
    fixture.cache_release("v99.0.0");
    let yesterday = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("current time")
        .as_millis()
        - 25 * 60 * 60 * 1000;
    fs::write(fixture.shown_marker(), yesterday.to_string()).expect("write shown marker");

    let stdout = fixture.render();
    assert!(
        stdout.contains("superline v99.0.0 available: cargo binstall superline"),
        "stdout:\n{stdout}"
    );
}

#[test]
fn the_running_version_and_older_releases_are_silent() {
    for version in [format!("v{CURRENT_VERSION}"), "v0.0.1".to_string()] {
        let fixture = Fixture::new("silent", "brew upgrade superline");
        fixture.cache_release(&version);

        let stdout = fixture.render();
        assert!(!stdout.contains("available"), "stdout:\n{stdout}");
        assert!(
            !fixture.shown_marker().exists(),
            "nothing was shown, so nothing should be recorded"
        );
    }
}
