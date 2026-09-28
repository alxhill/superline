//! `superline upgrade`: replaces the running binary with the prebuilt one
//! attached to a GitHub release.
//!
//! The archives are the ones `release-assets.yml` builds for every release,
//! the same files `cargo binstall` downloads. Each is downloaded in-process
//! (see [`crate::http`]), checked against the SHA-256 digest GitHub reports for
//! it, unpacked, run once to confirm it works on this machine, and only then
//! moved over the running binary.
//!
//! With `update.auto` on, [`AutoUpgrade`] runs the same install from the
//! detached refresh child once the daily check finds a newer release, and
//! records its [`Progress`] for the prompts drawn meanwhile.

use std::ffi::OsStr;
use std::fmt;
use std::fs;
use std::io::{self, Read};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use thiserror::Error;

use crate::cache::{write_entry, Cached, Source};
use crate::update::{github_api, is_newer, parse_version, CURRENT_VERSION, REPO};

const BIN_NAME: &str = if cfg!(windows) {
    "superline.exe"
} else {
    "superline"
};
const DOWNLOAD_TIMEOUT: Duration = Duration::from_secs(300);
/// The archives are around 3 MB; this only guards against a runaway response.
const DOWNLOAD_LIMIT: u64 = 64 * 1024 * 1024;
/// How often an automatic upgrade records how much it has downloaded.
const DOWNLOAD_PROGRESS_INTERVAL: Duration = Duration::from_millis(250);
/// No step of an automatic upgrade outlasts the download's timeout, so
/// progress this old was left behind by an upgrade that died partway.
pub(crate) const PROGRESS_TIMEOUT: Duration = Duration::from_secs(DOWNLOAD_TIMEOUT.as_secs() + 60);

#[derive(Debug, Error)]
pub enum UpgradeError {
    #[error("superline was installed with Homebrew, run `brew upgrade superline` instead")]
    Homebrew,
    #[error(
        "no prebuilt superline binary is published for this platform, run `cargo install superline` instead"
    )]
    UnsupportedPlatform,
    #[error("could not locate the running superline binary: {0}")]
    CurrentExe(io::Error),
    #[error("could not look up {what} on GitHub: {reason}")]
    ReleaseLookup { what: String, reason: String },
    #[error(
        "release {tag} has no {asset} yet. Its binaries may still be uploading, try again in a few minutes"
    )]
    MissingAsset { tag: String, asset: String },
    #[error("download failed: {0}")]
    Download(String),
    #[error("the download does not match its checksum (expected {expected}, got {actual})")]
    Checksum { expected: String, actual: String },
    #[error("could not unpack the download: {0}")]
    Extract(String),
    #[error("the downloaded binary does not run on this machine: {0}")]
    BrokenBinary(String),
    #[error("could not replace {}: {source}", path.display())]
    Replace { path: PathBuf, source: io::Error },
    #[error(transparent)]
    Io(#[from] io::Error),
}

impl UpgradeError {
    /// Failures worth retrying soon: the network, or a release whose binaries
    /// are still being uploaded.
    fn is_transient(&self) -> bool {
        matches!(
            self,
            UpgradeError::ReleaseLookup { .. }
                | UpgradeError::MissingAsset { .. }
                | UpgradeError::Download(_)
        )
    }

    /// Why a transient failure is retried, short enough for a prompt notice.
    fn retry_reason(&self) -> String {
        match self {
            UpgradeError::MissingAsset { .. } => "its binaries are still uploading".to_string(),
            error => error.to_string(),
        }
    }
}

/// A published release and its downloadable archives.
#[derive(Debug)]
pub struct Release {
    /// The release tag, `v0.16.0` style.
    pub tag: String,
    /// The release's web page.
    pub url: String,
    assets: Vec<Asset>,
}

#[derive(Debug, Deserialize)]
struct Asset {
    name: String,
    browser_download_url: String,
    /// `sha256:<hex>`. Older assets predate GitHub computing digests.
    #[serde(default)]
    digest: Option<String>,
    /// In bytes.
    #[serde(default)]
    size: Option<u64>,
}

impl Release {
    /// The version without the tag's `v` prefix, as `--version` prints it.
    pub fn version(&self) -> &str {
        self.tag.trim_start_matches('v')
    }
}

/// Looks up `version`, or the latest release when it is `None`.
pub fn find_release(version: Option<&str>) -> Result<Release, UpgradeError> {
    let (endpoint, description) = match version {
        Some(version) => {
            let tag = format!("v{}", version.trim_start_matches('v'));
            (
                format!("repos/{REPO}/releases/tags/{tag}"),
                format!("release {tag}"),
            )
        }
        None => (
            format!("repos/{REPO}/releases/latest"),
            "the latest release".to_string(),
        ),
    };
    let lookup_error = |reason: String| UpgradeError::ReleaseLookup {
        what: description.clone(),
        reason,
    };
    let json = github_api(&endpoint).map_err(|error| {
        lookup_error(match error {
            ureq::Error::StatusCode(404) => "it does not exist".to_string(),
            ureq::Error::StatusCode(403 | 429) => "GitHub's API rate limit was hit, try again in \
                 an hour or set GH_TOKEN"
                .to_string(),
            error => error.to_string(),
        })
    })?;
    parse_release(&json).ok_or_else(|| lookup_error("unexpected response".to_string()))
}

#[derive(Deserialize)]
struct GitHubRelease {
    tag_name: String,
    html_url: String,
    #[serde(default)]
    assets: Vec<Asset>,
}

fn parse_release(json: &[u8]) -> Option<Release> {
    let release: GitHubRelease = serde_json::from_slice(json).ok()?;
    parse_version(&release.tag_name)?;
    Some(Release {
        tag: release.tag_name,
        url: release.html_url,
        assets: release.assets,
    })
}

/// Whether installing `release` changes anything. Without an explicit version
/// only a newer release is installed, so a build ahead of the latest release
/// is left alone; an explicit version may also be a downgrade.
pub fn should_install(release: &Release, explicit: bool, force: bool) -> bool {
    should_install_version(release.version(), CURRENT_VERSION, explicit, force)
}

fn should_install_version(version: &str, current: &str, explicit: bool, force: bool) -> bool {
    if force {
        true
    } else if explicit {
        parse_version(version) != parse_version(current)
    } else {
        is_newer(version, current)
    }
}

/// The running binary, when it can be replaced by a release download.
pub struct Installation {
    exe: PathBuf,
    target: &'static str,
}

impl Installation {
    pub fn current() -> Result<Self, UpgradeError> {
        let exe = std::env::current_exe()
            .and_then(|exe| exe.canonicalize())
            .map_err(UpgradeError::CurrentExe)?;
        if is_homebrew(&exe) {
            return Err(UpgradeError::Homebrew);
        }
        let target = release_target().ok_or(UpgradeError::UnsupportedPlatform)?;
        Ok(Installation { exe, target })
    }

    /// The file that gets replaced, with symlinks resolved.
    pub fn exe(&self) -> &Path {
        &self.exe
    }

    /// The target triple of the archive that gets downloaded.
    pub fn target(&self) -> &'static str {
        self.target
    }

    /// Downloads `release`'s archive for this platform and moves its binary
    /// over the running one, calling `report` as each step starts and as the
    /// download arrives.
    pub fn install(
        &self,
        release: &Release,
        mut report: impl FnMut(Step),
    ) -> Result<(), UpgradeError> {
        let name = asset_name(release.version(), self.target);
        let asset = release
            .assets
            .iter()
            .find(|asset| asset.name == name)
            .ok_or_else(|| UpgradeError::MissingAsset {
                tag: release.tag.clone(),
                asset: name.clone(),
            })?;

        check_writable(&self.exe)?;
        let total = asset.size;
        report(Step::Downloading { received: 0, total });
        let archive = crate::http::download(
            &asset.browser_download_url,
            &[],
            DOWNLOAD_TIMEOUT,
            DOWNLOAD_LIMIT,
            |received| report(Step::Downloading { received, total }),
        )
        .map_err(|error| UpgradeError::Download(error.to_string()))?;
        report(Step::Verifying);
        if let Some(expected) = asset.digest.as_deref().and_then(sha256_hex) {
            let actual = hex(&Sha256::digest(&archive));
            if actual != expected {
                return Err(UpgradeError::Checksum {
                    expected: expected.to_string(),
                    actual,
                });
            }
        }
        let staging = Staging::create()?;
        let binary = staging.path().join(BIN_NAME);
        extract_binary(&archive[..], &binary)?;
        check_runs(&binary, release.version())?;
        report(Step::Installing);
        replace(&self.exe, &binary)
    }
}

/// A step of [`Installation::install`].
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Step {
    /// `received` bytes of the archive have arrived, out of `total` when
    /// GitHub reports its size.
    Downloading { received: u64, total: Option<u64> },
    /// Checking the archive against its checksum and test-running the binary
    /// in it.
    Verifying,
    /// Moving the new binary over the running one.
    Installing,
}

impl fmt::Display for Step {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match *self {
            Step::Downloading {
                received,
                total: Some(total),
            } if total > 0 => write!(
                f,
                "downloading {}/{} MB ({}%)",
                megabytes(received),
                megabytes(total),
                (received.min(total) * 100) / total
            ),
            Step::Downloading { received, .. } => {
                write!(f, "downloading {} MB", megabytes(received))
            }
            Step::Verifying => f.write_str("verifying the download"),
            Step::Installing => f.write_str("installing"),
        }
    }
}

fn megabytes(bytes: u64) -> String {
    format!("{:.1}", bytes as f64 / 1_000_000.0)
}

/// Whether this binary installs new releases itself when `update.auto` is on.
/// Only the prebuilt release binaries do, so a build from source, such as a
/// `cargo install --path .` under development, is never swapped out from
/// under its developer; it keeps showing the notice until `superline upgrade`
/// replaces it with a prebuilt one.
pub fn auto_upgrades() -> bool {
    option_env!("SUPERLINE_RELEASE_BUILD").is_some()
}

/// Installs the release tagged `tag` from the detached refresh child. There
/// is one cache entry per release, so the binary it installs can find the
/// entry for its own version and announce the upgrade.
#[derive(Clone, Serialize, Deserialize)]
pub struct AutoUpgrade {
    pub tag: String,
}

/// Where an automatic upgrade has got to. The refresh child keeps it in the
/// file at [`progress_path`] while it works, and removes it once the outcome
/// is cached.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Progress {
    FindingRelease,
    Step(Step),
    /// The attempt hit a failure that may clear up by itself, such as the
    /// release's binaries still uploading, and is tried again after
    /// [`AutoUpgrade::REFRESH_INTERVAL`].
    Retrying {
        reason: String,
    },
}

impl fmt::Display for Progress {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Progress::FindingRelease => f.write_str("looking up the release"),
            Progress::Step(step) => step.fmt(f),
            Progress::Retrying { reason } => write!(f, "retrying later: {reason}"),
        }
    }
}

/// The progress file for the [`AutoUpgrade`] cache entry at `cache_path`.
pub fn progress_path(cache_path: &Path) -> PathBuf {
    cache_path.with_extension("progress")
}

/// Records an automatic upgrade's [`Progress`], at most once every
/// [`DOWNLOAD_PROGRESS_INTERVAL`] while it downloads.
struct ProgressFile {
    path: Option<PathBuf>,
    last_download: Option<Instant>,
}

impl ProgressFile {
    fn report(&mut self, progress: Progress) {
        let Some(path) = &self.path else {
            return;
        };
        if let Progress::Step(Step::Downloading { .. }) = progress {
            let now = Instant::now();
            if self
                .last_download
                .is_some_and(|last| now - last < DOWNLOAD_PROGRESS_INTERVAL)
            {
                return;
            }
            self.last_download = Some(now);
        }
        write_entry(path, &progress);
    }

    fn clear(&self) {
        if let Some(path) = &self.path {
            let _ = fs::remove_file(path);
        }
    }
}

/// How an automatic upgrade went.
#[derive(Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum AutoUpgradeOutcome {
    Installed {
        /// The version that was replaced.
        from: String,
        /// The new release's web page.
        url: String,
    },
    Failed {
        error: String,
    },
}

impl Source for AutoUpgrade {
    type Value = AutoUpgradeOutcome;
    const KIND: &'static str = "auto-upgrade";
    /// A failed upgrade is tried again the next day.
    const TTL: Duration = Duration::from_secs(24 * 60 * 60);
    /// A transient failure, or an install that never finished, is retried
    /// after an hour.
    const REFRESH_INTERVAL: Duration = Duration::from_secs(60 * 60);

    fn cache_id(&self) -> String {
        self.tag.clone()
    }

    fn fetchable(&self) -> bool {
        Installation::current().is_ok()
    }

    fn fetch(&self) -> Option<AutoUpgradeOutcome> {
        let mut progress = ProgressFile {
            path: Cached::new(self.clone()).path().map(progress_path),
            last_download: None,
        };
        progress.report(Progress::FindingRelease);
        let result = find_release(Some(&self.tag)).and_then(|release| {
            Installation::current()?
                .install(&release, |step| progress.report(Progress::Step(step)))?;
            Ok(release.url)
        });
        match result {
            Ok(url) => {
                progress.clear();
                Some(AutoUpgradeOutcome::Installed {
                    from: CURRENT_VERSION.to_string(),
                    url,
                })
            }
            Err(error) if error.is_transient() => {
                progress.report(Progress::Retrying {
                    reason: error.retry_reason(),
                });
                None
            }
            Err(error) => {
                progress.clear();
                Some(AutoUpgradeOutcome::Failed {
                    error: error.to_string(),
                })
            }
        }
    }
}

/// Homebrew keeps its installs under a `Cellar` directory and tracks them
/// itself, so replacing the binary behind its back would confuse it.
pub(crate) fn is_homebrew(exe: &Path) -> bool {
    exe.components()
        .any(|component| component.as_os_str() == "Cellar")
}

/// The release archive that runs on this machine, from the targets
/// `release-assets.yml` builds.
pub(crate) fn release_target() -> Option<&'static str> {
    if cfg!(all(target_os = "macos", target_arch = "aarch64")) {
        Some("aarch64-apple-darwin")
    } else if cfg!(all(target_os = "linux", target_arch = "x86_64")) {
        // The gnu build targets x86-64-v3; the static musl build is the one for
        // musl distros and CPUs without AVX2.
        if cfg!(target_env = "gnu") && supports_x86_64_v3() {
            Some("x86_64-unknown-linux-gnu")
        } else {
            Some("x86_64-unknown-linux-musl")
        }
    } else if cfg!(all(target_os = "linux", target_arch = "aarch64")) {
        if cfg!(target_env = "gnu") {
            Some("aarch64-unknown-linux-gnu")
        } else {
            Some("aarch64-unknown-linux-musl")
        }
    } else if cfg!(all(
        target_os = "linux",
        target_arch = "arm",
        target_abi = "eabihf"
    )) {
        arm_release_target(&machine()?)
    } else if cfg!(all(target_os = "windows", target_arch = "x86_64")) {
        supports_x86_64_v3().then_some("x86_64-pc-windows-msvc")
    } else {
        None
    }
}

/// The 32-bit ARM build for a `uname -m` machine name. Rust's `cfg` cannot
/// tell ARMv6 from ARMv7, but the kernel can: a Pi 1 or Zero reports `armv6l`,
/// and later boards `armv7l`, or `armv8l` when a 64-bit CPU runs a 32-bit OS.
fn arm_release_target(machine: &str) -> Option<&'static str> {
    let version: u32 = machine
        .strip_prefix("armv")?
        .trim_end_matches(|c: char| c.is_ascii_alphabetic())
        .parse()
        .ok()?;
    match version {
        6 => Some("arm-unknown-linux-musleabihf"),
        7.. => Some("armv7-unknown-linux-musleabihf"),
        _ => None,
    }
}

/// The kernel's machine name, as `uname -m` prints it.
#[cfg(unix)]
fn machine() -> Option<String> {
    // SAFETY: `uname` fills the zeroed struct with NUL-terminated strings.
    let name = unsafe {
        let mut name: libc::utsname = std::mem::zeroed();
        if libc::uname(&mut name) != 0 {
            return None;
        }
        name
    };
    let machine = unsafe { std::ffi::CStr::from_ptr(name.machine.as_ptr()) };
    Some(machine.to_string_lossy().into_owned())
}

#[cfg(not(unix))]
fn machine() -> Option<String> {
    None
}

#[cfg(target_arch = "x86_64")]
fn supports_x86_64_v3() -> bool {
    is_x86_feature_detected!("avx2")
        && is_x86_feature_detected!("bmi1")
        && is_x86_feature_detected!("bmi2")
        && is_x86_feature_detected!("fma")
        && is_x86_feature_detected!("lzcnt")
        && is_x86_feature_detected!("movbe")
        && is_x86_feature_detected!("f16c")
}

#[cfg(not(target_arch = "x86_64"))]
fn supports_x86_64_v3() -> bool {
    false
}

/// Matches the name `release-assets.yml` packages each target as.
fn asset_name(version: &str, target: &str) -> String {
    format!("superline-{version}-{target}.tar.gz")
}

/// The hex digest from GitHub's `sha256:<hex>` form.
fn sha256_hex(digest: &str) -> Option<&str> {
    digest.strip_prefix("sha256:")
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

/// Writes the archive's `superline` binary to `dest`. `release-assets.yml`
/// packs it alone at the archive root; nothing else is unpacked.
fn extract_binary(archive: impl Read, dest: &Path) -> Result<(), UpgradeError> {
    let extract_error = |error: io::Error| UpgradeError::Extract(error.to_string());
    let mut archive = tar::Archive::new(flate2::read::GzDecoder::new(archive));
    for entry in archive.entries().map_err(extract_error)? {
        let mut entry = entry.map_err(extract_error)?;
        if entry.path().map_err(extract_error)?.as_os_str() == BIN_NAME {
            entry.unpack(dest).map_err(extract_error)?;
            return Ok(());
        }
    }
    Err(UpgradeError::Extract(format!(
        "the archive has no {BIN_NAME}"
    )))
}

/// Runs the new binary once, which catches a build this CPU or libc cannot
/// run, and checks it is the version that was asked for.
fn check_runs(binary: &Path, version: &str) -> Result<(), UpgradeError> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(binary, fs::Permissions::from_mode(0o755))?;
    }
    let output = Command::new(binary)
        .arg("--version")
        .stdin(Stdio::null())
        .output()
        .map_err(|error| UpgradeError::BrokenBinary(error.to_string()))?;
    if !output.status.success() {
        return Err(UpgradeError::BrokenBinary(format!(
            "`superline --version` exited with {}",
            output.status
        )));
    }
    let reported = String::from_utf8_lossy(&output.stdout);
    let expected = format!("superline {version}");
    if reported.trim() == expected {
        Ok(())
    } else {
        Err(UpgradeError::BrokenBinary(format!(
            "it reports `{}`, expected `{expected}`",
            reported.trim()
        )))
    }
}

/// Where the new binary is staged next to `exe`, so the final step is a
/// rename within one directory: a prompt that starts meanwhile runs either the
/// old binary or the new one, never half of one.
fn staged_path(exe: &Path) -> Result<PathBuf, UpgradeError> {
    let dir = exe.parent().ok_or_else(|| UpgradeError::Replace {
        path: exe.to_path_buf(),
        source: io::Error::new(io::ErrorKind::NotFound, "it has no parent directory"),
    })?;
    let mut name = OsStr::new(".").to_os_string();
    name.push(exe.file_name().unwrap_or(OsStr::new(BIN_NAME)));
    name.push(format!(".upgrade-{}", std::process::id()));
    Ok(dir.join(name))
}

/// Fails early, before anything is downloaded, when the binary's directory
/// cannot be written to.
fn check_writable(exe: &Path) -> Result<(), UpgradeError> {
    let staged = staged_path(exe)?;
    fs::File::create(&staged).map_err(|source| UpgradeError::Replace {
        path: exe.to_path_buf(),
        source,
    })?;
    let _ = fs::remove_file(&staged);
    Ok(())
}

/// Moves `new` over `exe` by way of [`staged_path`].
fn replace(exe: &Path, new: &Path) -> Result<(), UpgradeError> {
    let staged = staged_path(exe)?;
    let result = fs::copy(new, &staged)
        .and_then(|_| keep_permissions(exe, &staged))
        .and_then(|()| swap(exe, &staged));
    let _ = fs::remove_file(&staged);
    result.map_err(|source| UpgradeError::Replace {
        path: exe.to_path_buf(),
        source,
    })
}

#[cfg(unix)]
fn keep_permissions(exe: &Path, staged: &Path) -> io::Result<()> {
    fs::set_permissions(staged, fs::metadata(exe)?.permissions())
}

#[cfg(not(unix))]
fn keep_permissions(_exe: &Path, _staged: &Path) -> io::Result<()> {
    Ok(())
}

#[cfg(not(windows))]
fn swap(exe: &Path, staged: &Path) -> io::Result<()> {
    fs::rename(staged, exe)
}

/// Windows refuses to overwrite a running executable but lets it be renamed,
/// so the old binary steps aside first. It cannot be deleted while it runs, so
/// it is left as `<name>.old` and cleared by the next upgrade.
#[cfg(windows)]
fn swap(exe: &Path, staged: &Path) -> io::Result<()> {
    let mut old = exe.as_os_str().to_os_string();
    old.push(".old");
    let old = PathBuf::from(old);
    let _ = fs::remove_file(&old);
    fs::rename(exe, &old)?;
    fs::rename(staged, exe).inspect_err(|_| {
        let _ = fs::rename(&old, exe);
    })?;
    let _ = fs::remove_file(&old);
    Ok(())
}

/// A scratch directory for the download, removed when dropped.
struct Staging(PathBuf);

impl Staging {
    fn create() -> io::Result<Self> {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|duration| duration.subsec_nanos())
            .unwrap_or(0);
        let dir =
            std::env::temp_dir().join(format!("superline-upgrade-{}-{nanos}", std::process::id()));
        fs::create_dir_all(&dir)?;
        Ok(Staging(dir))
    }

    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for Staging {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const RELEASE_JSON: &[u8] = br#"{
        "tag_name": "v0.21.0",
        "html_url": "https://github.com/alxhill/superline/releases/tag/v0.21.0",
        "assets": [
            {
                "name": "superline-0.21.0-aarch64-apple-darwin.tar.gz",
                "browser_download_url": "https://github.com/alxhill/superline/releases/download/v0.21.0/superline-0.21.0-aarch64-apple-darwin.tar.gz",
                "digest": "sha256:3034cab47b317173f9a938076167dcd1a060e2f1563c98dd2fe3d41953b37bac",
                "size": 2606303
            },
            {
                "name": "superline-0.21.0-x86_64-unknown-linux-musl.tar.gz",
                "browser_download_url": "https://example.com/musl.tar.gz",
                "digest": null
            }
        ]
    }"#;

    #[test]
    fn release_json_keeps_the_assets_and_their_digests() {
        let release = parse_release(RELEASE_JSON).expect("release should parse");
        assert_eq!(release.tag, "v0.21.0");
        assert_eq!(release.version(), "0.21.0");
        assert_eq!(release.assets.len(), 2);

        let mac = &release.assets[0];
        assert_eq!(mac.name, asset_name("0.21.0", "aarch64-apple-darwin"));
        assert_eq!(
            mac.digest.as_deref().and_then(sha256_hex),
            Some("3034cab47b317173f9a938076167dcd1a060e2f1563c98dd2fe3d41953b37bac")
        );
        assert_eq!(mac.size, Some(2606303));
        assert_eq!(release.assets[1].digest, None);
        assert_eq!(release.assets[1].size, None);

        assert!(parse_release(br#"{"tag_name":"nightly","html_url":"x"}"#).is_none());
        assert!(parse_release(br#"{"message":"Not Found"}"#).is_none());
    }

    #[test]
    fn a_release_without_assets_still_parses() {
        let release = parse_release(br#"{"tag_name":"v0.21.0","html_url":"x"}"#)
            .expect("release should parse");
        assert!(release.assets.is_empty());
    }

    #[test]
    fn only_newer_releases_install_unless_asked_for() {
        assert!(should_install_version("0.21.0", "0.20.2", false, false));
        assert!(!should_install_version("0.20.2", "0.20.2", false, false));
        assert!(!should_install_version("0.20.1", "0.20.2", false, false));

        // An explicit version may be a downgrade, but not a no-op.
        assert!(should_install_version("0.20.1", "0.20.2", true, false));
        assert!(!should_install_version("0.20.2", "0.20.2", true, false));

        assert!(should_install_version("0.20.2", "0.20.2", false, true));
        assert!(should_install_version("0.20.1", "0.20.2", false, true));
    }

    #[test]
    fn digests_are_lowercase_hex() {
        assert_eq!(
            hex(&Sha256::digest(b"abc")),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
        assert_eq!(hex(&[0x00, 0x0f, 0xa0, 0xff]), "000fa0ff");
        assert_eq!(sha256_hex("sha512:abc"), None);
    }

    #[test]
    fn only_lasting_failures_are_cached() {
        assert!(UpgradeError::MissingAsset {
            tag: "v0.21.0".into(),
            asset: "superline-0.21.0-aarch64-apple-darwin.tar.gz".into(),
        }
        .is_transient());
        assert!(UpgradeError::Download("timed out".into()).is_transient());
        assert!(UpgradeError::ReleaseLookup {
            what: "release v0.21.0".into(),
            reason: "timed out".into(),
        }
        .is_transient());
        assert!(!UpgradeError::Homebrew.is_transient());
        assert!(!UpgradeError::BrokenBinary("SIGILL".into()).is_transient());
        assert!(!UpgradeError::Checksum {
            expected: "a".into(),
            actual: "b".into(),
        }
        .is_transient());
    }

    #[test]
    fn auto_upgrade_outcomes_round_trip_through_the_cache() {
        let installed = AutoUpgradeOutcome::Installed {
            from: "0.20.2".into(),
            url: "https://github.com/alxhill/superline/releases/tag/v0.21.0".into(),
        };
        let json = serde_json::to_string(&installed).unwrap();
        assert_eq!(
            json,
            r#"{"installed":{"from":"0.20.2","url":"https://github.com/alxhill/superline/releases/tag/v0.21.0"}}"#
        );
        assert_eq!(
            serde_json::from_str::<AutoUpgradeOutcome>(&json).unwrap(),
            installed
        );
        assert_eq!(
            AutoUpgrade {
                tag: "v0.21.0".into()
            }
            .cache_id(),
            "v0.21.0"
        );
    }

    #[test]
    fn steps_describe_how_far_the_install_has_got() {
        let downloading = |received, total| Step::Downloading { received, total };
        assert_eq!(
            downloading(0, Some(2_606_303)).to_string(),
            "downloading 0.0/2.6 MB (0%)"
        );
        assert_eq!(
            downloading(1_200_000, Some(2_606_303)).to_string(),
            "downloading 1.2/2.6 MB (46%)"
        );
        assert_eq!(
            downloading(2_606_303, Some(2_606_303)).to_string(),
            "downloading 2.6/2.6 MB (100%)"
        );
        assert_eq!(
            downloading(1_200_000, None).to_string(),
            "downloading 1.2 MB"
        );
        assert_eq!(downloading(0, Some(0)).to_string(), "downloading 0.0 MB");
        assert_eq!(Step::Verifying.to_string(), "verifying the download");
        assert_eq!(
            Progress::FindingRelease.to_string(),
            "looking up the release"
        );
        assert_eq!(Progress::Step(Step::Installing).to_string(), "installing");
    }

    #[test]
    fn progress_round_trips_through_its_file() {
        let progress = Progress::Step(Step::Downloading {
            received: 1_200_000,
            total: Some(2_606_303),
        });
        let json = serde_json::to_string(&progress).unwrap();
        assert_eq!(
            json,
            r#"{"step":{"downloading":{"received":1200000,"total":2606303}}}"#
        );
        assert_eq!(serde_json::from_str::<Progress>(&json).unwrap(), progress);
        assert_eq!(
            progress_path(Path::new("/cache/superline/auto-upgrade-v0.21.0.json")),
            Path::new("/cache/superline/auto-upgrade-v0.21.0.progress")
        );
    }

    #[test]
    fn missing_binaries_are_retried_with_a_short_reason() {
        let missing = UpgradeError::MissingAsset {
            tag: "v0.21.0".into(),
            asset: "superline-0.21.0-aarch64-apple-darwin.tar.gz".into(),
        };
        assert_eq!(missing.retry_reason(), "its binaries are still uploading");
        assert_eq!(
            UpgradeError::Download("timed out".into()).retry_reason(),
            "download failed: timed out"
        );
    }

    fn archive(entries: &[(&str, &[u8])]) -> Vec<u8> {
        let encoder = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::fast());
        let mut builder = tar::Builder::new(encoder);
        for (path, contents) in entries {
            let mut header = tar::Header::new_gnu();
            header.set_size(contents.len() as u64);
            header.set_mode(0o755);
            header.set_cksum();
            builder.append_data(&mut header, path, *contents).unwrap();
        }
        builder.into_inner().unwrap().finish().unwrap()
    }

    #[test]
    fn only_the_binary_is_unpacked_from_the_archive() {
        let dir =
            std::env::temp_dir().join(format!("superline-extract-test-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let dest = dir.join("unpacked");

        let release = archive(&[("README", b"hello"), (BIN_NAME, b"binary")]);
        extract_binary(&release[..], &dest).expect("the binary should unpack");
        assert_eq!(fs::read(&dest).unwrap(), b"binary");
        assert!(!dir.join("README").exists());

        let empty = archive(&[("README", b"hello")]);
        assert!(matches!(
            extract_binary(&empty[..], &dir.join("missing")),
            Err(UpgradeError::Extract(_))
        ));
        assert!(matches!(
            extract_binary(&b"not a gzip"[..], &dir.join("garbage")),
            Err(UpgradeError::Extract(_))
        ));
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn homebrew_installs_are_recognised_by_their_cellar() {
        assert!(is_homebrew(Path::new(
            "/opt/homebrew/Cellar/superline/0.20.2/bin/superline"
        )));
        assert!(!is_homebrew(Path::new("/Users/me/.cargo/bin/superline")));
    }

    #[test]
    fn this_platform_has_a_release_target_matching_its_build() {
        let target = release_target();
        if cfg!(all(target_os = "macos", target_arch = "aarch64")) {
            assert_eq!(target, Some("aarch64-apple-darwin"));
        }
        if cfg!(all(
            target_os = "linux",
            target_arch = "x86_64",
            target_env = "musl"
        )) {
            assert_eq!(target, Some("x86_64-unknown-linux-musl"));
        }
        if cfg!(all(target_os = "macos", target_arch = "x86_64")) {
            assert_eq!(target, None);
        }
    }

    #[test]
    fn arm_boards_get_the_build_for_their_architecture() {
        assert_eq!(
            arm_release_target("armv6l"),
            Some("arm-unknown-linux-musleabihf")
        );
        assert_eq!(
            arm_release_target("armv7l"),
            Some("armv7-unknown-linux-musleabihf")
        );
        assert_eq!(
            arm_release_target("armv8l"),
            Some("armv7-unknown-linux-musleabihf")
        );
        assert_eq!(arm_release_target("armv5tel"), None);
        assert_eq!(arm_release_target("aarch64"), None);
        assert_eq!(arm_release_target("x86_64"), None);
    }

    #[cfg(unix)]
    #[test]
    fn the_machine_name_is_read_from_the_kernel() {
        let machine = machine().expect("uname should succeed");
        assert!(!machine.is_empty());
        if cfg!(target_arch = "aarch64") && cfg!(target_os = "linux") {
            assert_eq!(machine, "aarch64");
        }
    }

    #[cfg(unix)]
    #[test]
    fn replace_swaps_the_binary_and_keeps_its_permissions() {
        use std::os::unix::fs::PermissionsExt;

        let dir =
            std::env::temp_dir().join(format!("superline-upgrade-test-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let exe = dir.join("superline");
        let new = dir.join("downloaded");
        fs::write(&exe, "old").unwrap();
        fs::set_permissions(&exe, fs::Permissions::from_mode(0o750)).unwrap();
        fs::write(&new, "new").unwrap();

        replace(&exe, &new).expect("replace should succeed");

        assert_eq!(fs::read_to_string(&exe).unwrap(), "new");
        assert_eq!(
            fs::metadata(&exe).unwrap().permissions().mode() & 0o777,
            0o750
        );
        let leftovers: Vec<_> = fs::read_dir(&dir)
            .unwrap()
            .map(|entry| entry.unwrap().file_name())
            .collect();
        assert_eq!(leftovers.len(), 2, "{leftovers:?}");
        fs::remove_dir_all(&dir).unwrap();
    }
}
