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
//! instead (see [`crate::upgrade::AutoUpgrade`]). Every prompt drawn while
//! the install runs says how far it has got, and the binary it installs
//! announces the upgrade on its first prompt.

use std::path::{Path, PathBuf};
use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::cache::{claim_slot, read_entry, Cached, Entry, Lookup, Source};
use crate::colors::Color;
use crate::platform::resolve_binary;
use crate::terminal::{BgColor, FgColor, Hyperlink, Reset};
use crate::themes::DefaultColors;
use crate::upgrade::{
    auto_upgrades, is_homebrew, progress_path, release_target, AutoUpgrade, AutoUpgradeOutcome,
    Progress, PROGRESS_TIMEOUT,
};

pub(crate) const REPO: &str = "alxhill/superline";
pub(crate) const CURRENT_VERSION: &str = env!("CARGO_PKG_VERSION");
/// Once the notice has been shown it stays hidden for this long.
const NOTICE_INTERVAL: Duration = Duration::from_secs(24 * 60 * 60);
const FETCH_TIMEOUT: Duration = Duration::from_secs(10);
/// GitHub's release objects are a few kilobytes; this only guards against a
/// runaway response.
const API_RESPONSE_LIMIT: u64 = 1024 * 1024;

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

    fn fetch(&self) -> Option<Release> {
        fetch_latest_release()
    }
}

/// The notice line to print above the prompt, when a newer release is cached
/// and the notice has not been shown in the last day. Reading the cache also
/// schedules the daily background check.
///
/// With `auto` on, the notice instead follows an upgrade that can run: it
/// says one is starting, how far it has got on every prompt while it runs,
/// and that it finished on the new binary's first prompt.
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
    let link = Hyperlink {
        url: &release.url,
        label: &release.version,
    };

    let mut failure = None;
    if auto && auto_upgrades() {
        let upgrade = Cached::new(AutoUpgrade {
            tag: release.version.clone(),
        });
        let lookup = upgrade.load();
        let progress = upgrade
            .path()
            .and_then(|path| read_entry(&progress_path(path)));
        match auto_upgrade_state(lookup, progress) {
            AutoUpgradeState::Starting => {
                return Some(format!(
                    "{}superline {link} available, upgrading from v{CURRENT_VERSION} in the background",
                    icon::<S>(),
                ));
            }
            AutoUpgradeState::Running(progress) => {
                return Some(format!(
                    "{}superline upgrading to {link}: {progress}",
                    icon::<S>(),
                ));
            }
            AutoUpgradeState::Retrying { reason, retry_in } => {
                if !claim_slot(
                    &retry_marker(upgrade.path()?),
                    AutoUpgrade::REFRESH_INTERVAL,
                ) {
                    return None;
                }
                return Some(format!(
                    "{}superline {link} available, retrying the upgrade in {}: {reason}",
                    icon::<S>(),
                    minutes(retry_in),
                ));
            }
            AutoUpgradeState::Hidden => return None,
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
        "{}superline {link} available: {command}{failure}",
        icon::<S>(),
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
        "{}superline upgraded from v{from} to {link}",
        icon::<S>(),
        link = Hyperlink {
            url: &url,
            label: &format!("v{CURRENT_VERSION}"),
        },
    ))
}

/// The icon that opens every notice, followed by a reset so the text after
/// it is in the terminal's default colours, and the space before that text.
/// An empty icon leaves nothing at all.
fn icon<S: UpdateScheme>() -> String {
    let icon = S::update_icon();
    if icon.is_empty() {
        return String::new();
    }
    let fg = FgColor::from(S::update_fg());
    format!(
        "{bg}{fg} {icon} {attrs_off}{reset} ",
        bg = BgColor::from(S::update_bg()),
        attrs_off = fg.attrs_off(),
        reset = Reset,
    )
}

#[derive(Debug, PartialEq)]
enum AutoUpgradeState {
    /// The install was just handed to the refresh child, which has not
    /// reported any progress yet.
    Starting,
    /// The install is partway through.
    Running(Progress),
    /// The last attempt hit a failure that may clear up by itself, and the
    /// next one starts after `retry_in`.
    Retrying { reason: String, retry_in: Duration },
    /// Nothing worth saying: the install finished and this prompt is still
    /// the old binary's, or it died partway and waits to be retried.
    Hidden,
    /// The last attempt failed in a way retrying soon will not fix.
    Failed(String),
    /// This installation cannot upgrade itself.
    Unavailable,
}

/// Reads the state of an upgrade from its cached outcome and, while it has
/// none, from the progress the refresh child records.
fn auto_upgrade_state(
    lookup: Lookup<AutoUpgradeOutcome>,
    progress: Option<Entry<Progress>>,
) -> AutoUpgradeState {
    let progress = progress.map(|entry| (entry.age(), entry.value));
    match (lookup, progress) {
        (Lookup::Ready(AutoUpgradeOutcome::Installed { .. }), _) => AutoUpgradeState::Hidden,
        // A failed upgrade is retried a day later, and that attempt's
        // progress is newer news than the failure.
        (_, Some((age, progress @ (Progress::FindingRelease | Progress::Step(_)))))
            if age < PROGRESS_TIMEOUT =>
        {
            AutoUpgradeState::Running(progress)
        }
        (Lookup::Ready(AutoUpgradeOutcome::Failed { error }), _) => AutoUpgradeState::Failed(error),
        (Lookup::Unavailable, _) => AutoUpgradeState::Unavailable,
        (Lookup::Loading, None) => AutoUpgradeState::Starting,
        // The refresh slot is claimed before the progress is first written,
        // so once the progress is older than the slot it has been released,
        // and loading the upgrade has just started the next attempt.
        (Lookup::Loading, Some((age, _))) if age >= AutoUpgrade::REFRESH_INTERVAL => {
            AutoUpgradeState::Starting
        }
        (Lookup::Loading, Some((age, Progress::Retrying { reason }))) => {
            AutoUpgradeState::Retrying {
                reason,
                retry_in: AutoUpgrade::REFRESH_INTERVAL.saturating_sub(age),
            }
        }
        (Lookup::Loading, Some(_)) => AutoUpgradeState::Hidden,
    }
}

/// Records when a retried upgrade was last announced, so each retry is
/// announced once.
fn retry_marker(cache_path: &Path) -> PathBuf {
    cache_path.with_extension("retry-shown")
}

/// `duration` in whole minutes, rounded up.
fn minutes(duration: Duration) -> String {
    match duration.as_secs().div_ceil(60).max(1) {
        1 => "a minute".to_string(),
        minutes => format!("{minutes} minutes"),
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

fn fetch_latest_release() -> Option<Release> {
    parse_release(&github_api(&format!("repos/{REPO}/releases/latest")).ok()?)
}

/// The body of a successful GitHub REST API response. Anonymous requests are
/// limited to 60 an hour per IP address, so a `GH_TOKEN` or `GITHUB_TOKEN` in
/// the environment is sent along when there is one, as `gh` would.
pub(crate) fn github_api(endpoint: &str) -> Result<Vec<u8>, ureq::Error> {
    let authorization = github_token().map(|token| format!("Bearer {token}"));
    let mut headers = vec![("Accept", "application/vnd.github+json")];
    if let Some(authorization) = &authorization {
        headers.push(("Authorization", authorization));
    }
    crate::http::get(
        &format!("https://api.github.com/{endpoint}"),
        &headers,
        FETCH_TIMEOUT,
        API_RESPONSE_LIMIT,
    )
}

fn github_token() -> Option<String> {
    ["GH_TOKEN", "GITHUB_TOKEN"]
        .into_iter()
        .filter_map(|name| std::env::var(name).ok())
        .find(|token| !token.trim().is_empty())
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
    use crate::upgrade::Step;

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

    fn progress(age: Duration, progress: Progress) -> Option<Entry<Progress>> {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs();
        Some(Entry {
            fetched_at: now - age.as_secs(),
            value: progress,
        })
    }

    const DOWNLOADING: Progress = Progress::Step(Step::Downloading {
        received: 1_200_000,
        total: Some(2_600_000),
    });

    #[test]
    fn an_upgrade_is_starting_until_the_child_reports() {
        assert_eq!(
            auto_upgrade_state(Lookup::Loading, None),
            AutoUpgradeState::Starting
        );
    }

    #[test]
    fn a_running_upgrade_shows_its_latest_progress() {
        assert_eq!(
            auto_upgrade_state(
                Lookup::Loading,
                progress(Duration::from_secs(2), DOWNLOADING)
            ),
            AutoUpgradeState::Running(DOWNLOADING)
        );
        assert_eq!(
            auto_upgrade_state(
                Lookup::Loading,
                progress(Duration::ZERO, Progress::FindingRelease)
            ),
            AutoUpgradeState::Running(Progress::FindingRelease)
        );
        // The daily retry of a failed upgrade.
        assert_eq!(
            auto_upgrade_state(
                Lookup::Ready(AutoUpgradeOutcome::Failed {
                    error: "no space left".into(),
                }),
                progress(Duration::ZERO, Progress::Step(Step::Verifying))
            ),
            AutoUpgradeState::Running(Progress::Step(Step::Verifying))
        );
    }

    #[test]
    fn a_finished_or_failed_upgrade_ignores_its_progress() {
        assert_eq!(
            auto_upgrade_state(
                Lookup::Ready(AutoUpgradeOutcome::Installed {
                    from: "0.20.2".into(),
                    url: "x".into(),
                }),
                progress(Duration::ZERO, DOWNLOADING)
            ),
            AutoUpgradeState::Hidden
        );
        assert_eq!(
            auto_upgrade_state(
                Lookup::Ready(AutoUpgradeOutcome::Failed {
                    error: "no space left".into(),
                }),
                None
            ),
            AutoUpgradeState::Failed("no space left".into())
        );
        assert_eq!(
            auto_upgrade_state(Lookup::Unavailable, None),
            AutoUpgradeState::Unavailable
        );
    }

    #[test]
    fn a_transient_failure_says_when_it_is_retried() {
        let retrying = Progress::Retrying {
            reason: "its binaries are still uploading".into(),
        };
        assert_eq!(
            auto_upgrade_state(
                Lookup::Loading,
                progress(Duration::from_secs(15 * 60), retrying.clone())
            ),
            AutoUpgradeState::Retrying {
                reason: "its binaries are still uploading".into(),
                retry_in: Duration::from_secs(45 * 60),
            }
        );
        // Once the retry is due, loading the upgrade has just started it.
        assert_eq!(
            auto_upgrade_state(
                Lookup::Loading,
                progress(AutoUpgrade::REFRESH_INTERVAL, retrying)
            ),
            AutoUpgradeState::Starting
        );
    }

    #[test]
    fn an_upgrade_that_died_waits_quietly_for_its_retry() {
        assert_eq!(
            auto_upgrade_state(Lookup::Loading, progress(PROGRESS_TIMEOUT, DOWNLOADING)),
            AutoUpgradeState::Hidden
        );
        assert_eq!(
            auto_upgrade_state(
                Lookup::Loading,
                progress(AutoUpgrade::REFRESH_INTERVAL, DOWNLOADING)
            ),
            AutoUpgradeState::Starting
        );
    }

    #[test]
    fn retry_delays_round_up_to_whole_minutes() {
        assert_eq!(minutes(Duration::from_secs(45 * 60)), "45 minutes");
        assert_eq!(minutes(Duration::from_secs(44 * 60 + 1)), "45 minutes");
        assert_eq!(minutes(Duration::from_secs(30)), "a minute");
        assert_eq!(minutes(Duration::ZERO), "a minute");
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
