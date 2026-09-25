//! A once-a-day notice, printed above the prompt, that a newer superline
//! release is available.
//!
//! The latest release is looked up through the GitHub API in the background
//! and cached for a day, so the network is touched at most once a day. When
//! the cached release is newer than the running binary the notice is rendered
//! on one prompt and then suppressed for another day, using the same locked
//! marker file the cache uses to rate-limit refreshes.
//!
//! With `update.auto` on, a prebuilt binary installs the release itself
//! instead (see [`crate::upgrade::AutoUpgrade`]), and the binary it installs
//! announces the upgrade on its first prompt.

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::cache::{claim_slot, Cached, Lookup, Source};
use crate::colors::Color;
use crate::platform::resolve_binary;
use crate::terminal::{BgColor, FgColor, Hyperlink, Reset};
use crate::themes::DefaultColors;
use crate::upgrade::{auto_upgrades, is_homebrew, release_target, AutoUpgrade, AutoUpgradeOutcome};

pub(crate) const REPO: &str = "alxhill/superline";
pub(crate) const CURRENT_VERSION: &str = env!("CARGO_PKG_VERSION");
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
///
/// With `auto` on, the notice instead announces an upgrade that just
/// finished, and gives way to the install when one can run.
pub fn notice<S: UpdateScheme>(auto: bool) -> Option<String> {
    if auto {
        if let Some(notice) = upgraded_notice::<S>() {
            return Some(notice);
        }
    }

    let cached = Cached::new(UpdateLookup);
    let release = cached.load().ready()?;
    if !is_newer(&release.version, CURRENT_VERSION) {
        return None;
    }

    let mut failure = None;
    if auto && auto_upgrades() {
        let upgrade = Cached::new(AutoUpgrade {
            tag: release.version.clone(),
        });
        match auto_upgrade_state(upgrade.load()) {
            AutoUpgradeState::Running => return None,
            AutoUpgradeState::Failed(error) => failure = Some(error),
            AutoUpgradeState::Unavailable => {}
        }
    }
    if !claim_slot(&shown_marker(cached.path()?), NOTICE_INTERVAL) {
        return None;
    }

    let command = upgrade_command();
    let failure = failure
        .map(|error| format!(" (auto-upgrade failed: {error})"))
        .unwrap_or_default();
    Some(format!(
        "{} superline {link} available: {command}{failure}",
        icon::<S>(),
        link = Hyperlink {
            url: &release.url,
            label: &release.version,
        },
    ))
}

/// Announces, once, that this binary was just installed by an automatic
/// upgrade. The entry is removed as it is shown, and only the prompt whose
/// removal succeeds prints it, so concurrent prompts do not repeat it.
fn upgraded_notice<S: UpdateScheme>() -> Option<String> {
    let cached = Cached::new(AutoUpgrade {
        tag: format!("v{CURRENT_VERSION}"),
    });
    let AutoUpgradeOutcome::Installed { from, url } = cached.read()?.value else {
        return None;
    };
    std::fs::remove_file(cached.path()?).ok()?;
    Some(format!(
        "{} superline upgraded from v{from} to {link}",
        icon::<S>(),
        link = Hyperlink {
            url: &url,
            label: &format!("v{CURRENT_VERSION}"),
        },
    ))
}

/// The icon that opens every notice, followed by a reset so the text after
/// it is in the terminal's default colours.
fn icon<S: UpdateScheme>() -> String {
    format!(
        "{bg}{fg} {icon} {reset}",
        bg = BgColor::from(S::update_bg()),
        fg = FgColor::from(S::update_fg()),
        icon = S::update_icon(),
        reset = Reset,
    )
}

#[derive(Debug, PartialEq)]
enum AutoUpgradeState {
    /// An install is in flight, or finished and this prompt is still the old
    /// binary's; either way the next prompt knows more.
    Running,
    /// The last attempt failed in a way retrying soon will not fix.
    Failed(String),
    /// This installation cannot upgrade itself.
    Unavailable,
}

fn auto_upgrade_state(lookup: Lookup<AutoUpgradeOutcome>) -> AutoUpgradeState {
    match lookup {
        Lookup::Loading | Lookup::Ready(AutoUpgradeOutcome::Installed { .. }) => {
            AutoUpgradeState::Running
        }
        Lookup::Ready(AutoUpgradeOutcome::Failed { error }) => AutoUpgradeState::Failed(error),
        Lookup::Unavailable => AutoUpgradeState::Unavailable,
    }
}

/// Records when the notice was last shown, next to the cache entry so
/// `clear-caches` and the daily prune sweep cover it too.
fn shown_marker(cache_path: &Path) -> PathBuf {
    cache_path.with_extension("shown")
}

/// Whether `latest` is a higher version than `current`. Anything that does not
/// parse as a version is never newer, so a malformed tag stays silent.
pub(crate) fn is_newer(latest: &str, current: &str) -> bool {
    match (parse_version(latest), parse_version(current)) {
        (Some(latest), Some(current)) => latest > current,
        _ => false,
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct Version(u64, u64, u64);

/// Parses `1.2.3`, `v1.2.3` or `1.2.3-rc.1`, ignoring any pre-release or
/// build suffix. Missing minor and patch components count as zero.
pub(crate) fn parse_version(text: &str) -> Option<Version> {
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
/// binary lives: Homebrew keeps it under a `Cellar` directory, and anything
/// else can replace itself with a release download when one is published for
/// this platform. Failing that it came from cargo, where `cargo binstall` is
/// preferred when available.
fn upgrade_command() -> String {
    let exe = std::env::current_exe()
        .ok()
        .and_then(|exe| exe.canonicalize().ok());
    upgrade_command_for(
        exe.as_deref(),
        release_target().is_some(),
        resolve_binary("cargo-binstall").is_some(),
    )
    .to_string()
}

fn upgrade_command_for(exe: Option<&Path>, prebuilt: bool, has_binstall: bool) -> &'static str {
    if exe.is_some_and(is_homebrew) {
        "brew upgrade superline"
    } else if prebuilt {
        "superline upgrade"
    } else if has_binstall {
        "cargo binstall superline"
    } else {
        "cargo install superline"
    }
}

pub(crate) enum Fetcher {
    Curl(PathBuf),
    Gh(PathBuf),
}

/// `curl` ships with macOS, Windows 10+ and nearly every Linux; `gh` is the
/// fallback for anyone who has the PR module working but no curl.
pub(crate) fn fetcher() -> Option<Fetcher> {
    resolve_binary("curl")
        .map(Fetcher::Curl)
        .or_else(|| resolve_binary("gh").map(Fetcher::Gh))
}

fn fetch_latest_release() -> Option<Release> {
    parse_release(&github_api(&format!("repos/{REPO}/releases/latest"))?)
}

/// The body of a GitHub REST API response, or `None` when the request fails.
pub(crate) fn github_api(endpoint: &str) -> Option<Vec<u8>> {
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
            command.args(["api", endpoint]);
            command
        }
    };
    let output = command
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()
        .ok()?;
    output.status.success().then_some(output.stdout)
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
    fn a_running_or_finished_auto_upgrade_hides_the_notice() {
        assert_eq!(
            auto_upgrade_state(Lookup::Loading),
            AutoUpgradeState::Running
        );
        assert_eq!(
            auto_upgrade_state(Lookup::Ready(AutoUpgradeOutcome::Installed {
                from: "0.20.2".into(),
                url: "x".into(),
            })),
            AutoUpgradeState::Running
        );
        assert_eq!(
            auto_upgrade_state(Lookup::Ready(AutoUpgradeOutcome::Failed {
                error: "no space left".into(),
            })),
            AutoUpgradeState::Failed("no space left".into())
        );
        assert_eq!(
            auto_upgrade_state(Lookup::Unavailable),
            AutoUpgradeState::Unavailable
        );
    }

    #[test]
    fn upgrade_command_follows_the_install_location() {
        let brew = Path::new("/opt/homebrew/Cellar/superline/0.16.0/bin/superline");
        assert_eq!(
            upgrade_command_for(Some(brew), true, true),
            "brew upgrade superline"
        );
        let linuxbrew =
            Path::new("/home/linuxbrew/.linuxbrew/Cellar/superline/0.16.0/bin/superline");
        assert_eq!(
            upgrade_command_for(Some(linuxbrew), true, false),
            "brew upgrade superline"
        );

        let cargo = Path::new("/Users/me/.cargo/bin/superline");
        assert_eq!(
            upgrade_command_for(Some(cargo), true, true),
            "superline upgrade"
        );
        assert_eq!(upgrade_command_for(None, true, false), "superline upgrade");
        assert_eq!(
            upgrade_command_for(Some(cargo), false, true),
            "cargo binstall superline"
        );
        assert_eq!(
            upgrade_command_for(Some(cargo), false, false),
            "cargo install superline"
        );
        assert_eq!(
            upgrade_command_for(None, false, false),
            "cargo install superline"
        );
    }
}
