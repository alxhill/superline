//! A once-a-day notice, printed above the prompt, that a newer superline
//! release is available.
//!
//! The latest release is looked up through the GitHub API in the background
//! and cached for a day, so the network is touched at most once a day. When
//! the cached release is newer than the running binary the notice is rendered
//! on one prompt and then suppressed for another day, using the same locked
//! marker file the cache uses to rate-limit refreshes.

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::cache::{claim_slot, Cached, Source};
use crate::colors::Color;
use crate::platform::resolve_binary;
use crate::terminal::{BgColor, FgColor, Hyperlink, Reset};
use crate::themes::DefaultColors;

const REPO: &str = "alxhill/superline";
const CURRENT_VERSION: &str = env!("CARGO_PKG_VERSION");
/// Once the notice has been shown it stays hidden for this long.
const NOTICE_INTERVAL: Duration = Duration::from_secs(24 * 60 * 60);
const FETCH_TIMEOUT_SECS: &str = "10";

/// Colours for the icon that opens the notice. The text after it is printed
/// in the terminal's default colours.
pub trait UpdateScheme: DefaultColors {
    const DEFAULT_ICON: &'static str = "\u{f0aa}"; // nf-fa-arrow_circle_up

    fn update_fg() -> Color {
        Self::default_fg()
    }
    fn update_bg() -> Color {
        Self::default_bg()
    }
    fn update_icon() -> &'static str {
        Self::DEFAULT_ICON
    }
}

/// The newest published release.
#[derive(Serialize, Deserialize)]
pub struct Release {
    /// The release tag, `v0.16.0` style.
    pub version: String,
    /// The release's web page.
    pub url: String,
}

/// Looks up the latest superline release. It takes no parameters, so there is
/// a single cache entry shared by every prompt.
#[derive(Clone, Serialize, Deserialize)]
pub struct UpdateLookup;

impl Source for UpdateLookup {
    type Value = Release;
    const KIND: &'static str = "update";
    const TTL: Duration = Duration::from_secs(24 * 60 * 60);
    /// A failed lookup (offline, rate limited) is not retried for an hour.
    const REFRESH_INTERVAL: Duration = Duration::from_secs(60 * 60);

    fn cache_id(&self) -> String {
        "latest".to_string()
    }

    fn fetchable(&self) -> bool {
        fetcher().is_some()
    }

    fn fetch(&self) -> Option<Release> {
        fetch_latest_release()
    }
}

/// The notice line to print above the prompt, when a newer release is cached
/// and the notice has not been shown in the last day. Reading the cache also
/// schedules the daily background check.
pub fn notice<S: UpdateScheme>() -> Option<String> {
    let cached = Cached::new(UpdateLookup);
    let release = cached.load().ready()?;
    if !is_newer(&release.version, CURRENT_VERSION) {
        return None;
    }
    if !claim_slot(&shown_marker(cached.path()?), NOTICE_INTERVAL) {
        return None;
    }

    let command = upgrade_command();
    Some(format!(
        "{bg}{fg} {icon} {reset} superline {link} available: {command}",
        bg = BgColor::from(S::update_bg()),
        fg = FgColor::from(S::update_fg()),
        icon = S::update_icon(),
        reset = Reset,
        link = Hyperlink {
            url: &release.url,
            label: &release.version,
        },
    ))
}

/// Records when the notice was last shown, next to the cache entry so
/// `clear-caches` and the daily prune sweep cover it too.
fn shown_marker(cache_path: &Path) -> PathBuf {
    cache_path.with_extension("shown")
}

/// Whether `latest` is a higher version than `current`. Anything that does not
/// parse as a version is never newer, so a malformed tag stays silent.
fn is_newer(latest: &str, current: &str) -> bool {
    match (parse_version(latest), parse_version(current)) {
        (Some(latest), Some(current)) => latest > current,
        _ => false,
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
struct Version(u64, u64, u64);

/// Parses `1.2.3`, `v1.2.3` or `1.2.3-rc.1`, ignoring any pre-release or
/// build suffix. Missing minor and patch components count as zero.
fn parse_version(text: &str) -> Option<Version> {
    let core = text
        .trim()
        .trim_start_matches('v')
        .split(['-', '+'])
        .next()?;
    let mut numbers = core.split('.').map(|part| part.parse::<u64>().ok());
    let major = numbers.next()??;
    let minor = numbers.next().unwrap_or(Some(0))?;
    let patch = numbers.next().unwrap_or(Some(0))?;
    Some(Version(major, minor, patch))
}

/// The command that upgrades this installation, inferred from where the
/// binary lives: Homebrew keeps it under a `Cellar` directory, and otherwise it
/// came from cargo, where `cargo binstall` is preferred when available since
/// it downloads a prebuilt binary instead of compiling.
fn upgrade_command() -> String {
    let exe = std::env::current_exe()
        .ok()
        .and_then(|exe| exe.canonicalize().ok());
    upgrade_command_for(exe.as_deref(), resolve_binary("cargo-binstall").is_some()).to_string()
}

fn upgrade_command_for(exe: Option<&Path>, has_binstall: bool) -> &'static str {
    let homebrew = exe.is_some_and(|exe| {
        exe.components()
            .any(|component| component.as_os_str() == "Cellar")
    });
    if homebrew {
        "brew upgrade superline"
    } else if has_binstall {
        "cargo binstall superline"
    } else {
        "cargo install superline"
    }
}

enum Fetcher {
    Curl(PathBuf),
    Gh(PathBuf),
}

/// `curl` ships with macOS, Windows 10+ and nearly every Linux; `gh` is the
/// fallback for anyone who has the PR module working but no curl.
fn fetcher() -> Option<Fetcher> {
    resolve_binary("curl")
        .map(Fetcher::Curl)
        .or_else(|| resolve_binary("gh").map(Fetcher::Gh))
}

fn fetch_latest_release() -> Option<Release> {
    let endpoint = format!("repos/{REPO}/releases/latest");
    let mut command = match fetcher()? {
        Fetcher::Curl(curl) => {
            let mut command = Command::new(curl);
            command.args([
                "--silent",
                "--fail",
                "--location",
                "--max-time",
                FETCH_TIMEOUT_SECS,
                "--header",
                "Accept: application/vnd.github+json",
                "--header",
                &format!("User-Agent: superline/{CURRENT_VERSION}"),
                &format!("https://api.github.com/{endpoint}"),
            ]);
            command
        }
        Fetcher::Gh(gh) => {
            let mut command = Command::new(gh);
            command.args(["api", &endpoint]);
            command
        }
    };
    let output = command
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    parse_release(&output.stdout)
}

/// The fields of GitHub's release object the notice needs.
#[derive(Deserialize)]
struct GitHubRelease {
    tag_name: String,
    html_url: String,
}

/// Only releases whose tag parses as a version are cached, so the comparison
/// at render time can never be against something unreadable.
fn parse_release(json: &[u8]) -> Option<Release> {
    let release: GitHubRelease = serde_json::from_slice(json).ok()?;
    parse_version(&release.tag_name)?;
    Some(Release {
        version: release.tag_name,
        url: release.html_url,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn versions_parse_with_or_without_a_prefix_and_suffix() {
        assert_eq!(parse_version("v0.16.0"), Some(Version(0, 16, 0)));
        assert_eq!(parse_version("1.2.3"), Some(Version(1, 2, 3)));
        assert_eq!(parse_version("1.2.3-rc.1+build"), Some(Version(1, 2, 3)));
        assert_eq!(parse_version("2"), Some(Version(2, 0, 0)));
        assert_eq!(parse_version("nightly"), None);
        assert_eq!(parse_version("1.x"), None);
        assert_eq!(parse_version(""), None);
    }

    #[test]
    fn only_a_higher_version_is_newer() {
        assert!(is_newer("v0.17.0", "0.16.0"));
        assert!(is_newer("v1.0.0", "0.99.99"));
        assert!(!is_newer("v0.16.0", "0.16.0"));
        assert!(!is_newer("v0.15.9", "0.16.0"));
        assert!(!is_newer("latest", "0.16.0"));
        assert!(!is_newer("v0.17.0", "dev"));
    }

    #[test]
    fn release_json_keeps_the_tag_and_web_page() {
        let release = parse_release(
            br#"{"tag_name":"v0.17.0","html_url":"https://github.com/alxhill/superline/releases/tag/v0.17.0","name":"v0.17.0","draft":false}"#,
        )
        .expect("release should parse");
        assert_eq!(release.version, "v0.17.0");
        assert_eq!(
            release.url,
            "https://github.com/alxhill/superline/releases/tag/v0.17.0"
        );

        assert!(parse_release(br#"{"tag_name":"latest","html_url":"x"}"#).is_none());
        assert!(parse_release(br#"{"message":"Not Found"}"#).is_none());
        assert!(parse_release(b"<html>").is_none());
    }

    #[test]
    fn upgrade_command_follows_the_install_location() {
        let brew = Path::new("/opt/homebrew/Cellar/superline/0.16.0/bin/superline");
        assert_eq!(
            upgrade_command_for(Some(brew), true),
            "brew upgrade superline"
        );
        let linuxbrew =
            Path::new("/home/linuxbrew/.linuxbrew/Cellar/superline/0.16.0/bin/superline");
        assert_eq!(
            upgrade_command_for(Some(linuxbrew), false),
            "brew upgrade superline"
        );

        let cargo = Path::new("/Users/me/.cargo/bin/superline");
        assert_eq!(
            upgrade_command_for(Some(cargo), true),
            "cargo binstall superline"
        );
        assert_eq!(
            upgrade_command_for(Some(cargo), false),
            "cargo install superline"
        );
        assert_eq!(upgrade_command_for(None, false), "cargo install superline");
    }
}
