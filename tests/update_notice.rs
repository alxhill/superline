//! Drives the compiled binary against a pre-seeded update cache to check the
//! notice is printed above the prompt once, links to the release, and then
//! stays hidden.

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
    fn new(label: &str, update_block: &str) -> Self {
        let root =
            std::env::temp_dir().join(format!("superline-update-{label}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        let cache_dir = root.join("cache/superline");
        fs::create_dir_all(&cache_dir).expect("create cache directory");

        let config = root.join("config.json");
        fs::write(
            &config,
            format!(r#"{{"theme":"rainbow","rows":[{{"left":["shell"]}}]{update_block}}}"#),
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

    /// Seeds the entry an automatic upgrade to this binary's version leaves
    /// behind.
    fn cache_auto_upgrade(&self, from: &str) -> PathBuf {
        let path = self
            .cache_dir
            .join(format!("auto-upgrade-v{CURRENT_VERSION}.json"));
        fs::write(
            &path,
            format!(
                r#"{{"fetched_at":{},"value":{{"installed":{{"from":"{from}","url":"https://github.com/alxhill/superline/releases/tag/v{CURRENT_VERSION}"}}}}}}"#,
                SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .expect("current time")
                    .as_secs()
            ),
        )
        .expect("write auto-upgrade cache");
        path
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
fn a_newer_release_is_announced_once_above_the_prompt() {
    let fixture = Fixture::new("newer", "");
    fixture.cache_release("v99.0.0");

    let first = fixture.render();
    let mut lines = first.lines();
    let notice = lines.next().expect("the notice line");
    assert!(notice.contains("v99.0.0"), "stdout:\n{first}");
    assert!(
        ["superline upgrade", "cargo ", "brew "]
            .iter()
            .any(|command| notice.contains(&format!("available: {command}"))),
        "the notice should end with the upgrade command\nstdout:\n{first}"
    );
    assert!(
        notice.contains("https://github.com/alxhill/superline/releases/tag/v99.0.0"),
        "the notice should link to the release\nstdout:\n{first}"
    );
    // The icon sits on the theme's background (rainbow: nice_purple) and the
    // text after it is reset to the terminal's default colours.
    let icon_background = notice.find("\x1b[48;5;93m").expect("icon background");
    let reset = notice.find("\x1b[0m").expect("reset before the text");
    let text = notice.find(" superline ").expect("notice text");
    assert!(icon_background < reset && reset < text, "stdout:\n{first}");
    assert!(
        lines.next().is_some_and(|prompt| prompt.contains("fish")),
        "the prompt should follow on the next line\nstdout:\n{first}"
    );
    assert!(fixture.shown_marker().is_file());

    // The same day, the notice stays hidden even though the release is still
    // newer than this binary.
    let second = fixture.render();
    assert!(!second.contains("v99.0.0"), "stdout:\n{second}");
    assert!(second
        .lines()
        .next()
        .is_some_and(|line| line.contains("fish")));
}

#[test]
fn a_notice_shown_yesterday_is_shown_again() {
    let fixture = Fixture::new("yesterday", "");
    fixture.cache_release("v99.0.0");
    let yesterday = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("current time")
        .as_millis()
        - 25 * 60 * 60 * 1000;
    fs::write(fixture.shown_marker(), yesterday.to_string()).expect("write shown marker");

    let stdout = fixture.render();
    assert!(
        stdout.contains("superline \x1b]8;;https://github.com/alxhill/superline/releases/tag/v99.0.0\x1b\\v99.0.0\x1b]8;;\x1b\\ available: "),
        "stdout:\n{stdout}"
    );
}

#[test]
fn the_running_version_and_older_releases_are_silent() {
    for version in [format!("v{CURRENT_VERSION}"), "v0.0.1".to_string()] {
        let fixture = Fixture::new("silent", "");
        fixture.cache_release(&version);

        let stdout = fixture.render();
        assert!(!stdout.contains("available"), "stdout:\n{stdout}");
        assert!(
            !fixture.shown_marker().exists(),
            "nothing was shown, so nothing should be recorded"
        );
    }
}

#[test]
fn the_notice_can_be_disabled() {
    let fixture = Fixture::new("disabled", r#","update":{"disable":true}"#);
    fixture.cache_release("v99.0.0");

    let stdout = fixture.render();
    assert!(!stdout.contains("v99.0.0"), "stdout:\n{stdout}");
    assert!(!fixture.shown_marker().exists());
}

#[test]
fn a_finished_auto_upgrade_is_announced_once() {
    let fixture = Fixture::new("upgraded", "");
    fixture.cache_release(&format!("v{CURRENT_VERSION}"));
    let entry = fixture.cache_auto_upgrade("0.0.1");

    let first = fixture.render();
    let notice = first.lines().next().expect("the notice line");
    assert!(
        notice.contains(&format!(
            "superline upgraded from v0.0.1 to \x1b]8;;https://github.com/alxhill/superline/releases/tag/v{CURRENT_VERSION}\x1b\\v{CURRENT_VERSION}\x1b]8;;\x1b\\"
        )),
        "stdout:\n{first}"
    );
    assert!(!entry.exists(), "the announcement should be used up");

    let second = fixture.render();
    assert!(!second.contains("upgraded"), "stdout:\n{second}");
}

#[test]
fn a_finished_auto_upgrade_is_not_announced_with_auto_off() {
    let fixture = Fixture::new("upgraded-off", r#","update":{"auto":false}"#);
    fixture.cache_release(&format!("v{CURRENT_VERSION}"));
    let entry = fixture.cache_auto_upgrade("0.0.1");

    let stdout = fixture.render();
    assert!(!stdout.contains("upgraded"), "stdout:\n{stdout}");
    assert!(entry.exists());
}

#[test]
fn a_source_build_with_auto_on_still_shows_the_notice() {
    if option_env!("SUPERLINE_RELEASE_BUILD").is_some() {
        return;
    }
    let fixture = Fixture::new("auto-source", "");
    fixture.cache_release("v99.0.0");

    let stdout = fixture.render();
    assert!(stdout.contains("available: "), "stdout:\n{stdout}");
    assert!(
        !fixture
            .cache_dir
            .join("auto-upgrade-v99.0.0.refresh")
            .exists(),
        "a source build should never start an upgrade"
    );
}
