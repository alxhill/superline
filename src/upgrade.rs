//! `superline upgrade`: replaces the running binary with the prebuilt one
//! attached to a GitHub release.
//!
//! The archives are the ones `release-assets.yml` builds for every release,
//! the same files `cargo binstall` downloads. Each is fetched with `curl` (or
//! `gh`), checked against the SHA-256 digest GitHub reports for it, unpacked
//! with the system `tar`, run once to confirm it works on this machine, and
//! only then moved over the running binary.

use std::ffi::OsStr;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::Deserialize;
use sha2::{Digest, Sha256};
use thiserror::Error;

use crate::update::{fetcher, github_api, is_newer, parse_version, Fetcher, CURRENT_VERSION, REPO};

const BIN_NAME: &str = if cfg!(windows) {
    "superline.exe"
} else {
    "superline"
};
const DOWNLOAD_TIMEOUT_SECS: &str = "300";

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
    #[error("neither curl nor gh is on PATH, one is needed to download the release")]
    NoDownloader,
    #[error("could not look up {0} on GitHub")]
    ReleaseLookup(String),
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
    if fetcher().is_none() {
        return Err(UpgradeError::NoDownloader);
    }
    github_api(&endpoint)
        .and_then(|json| parse_release(&json))
        .ok_or(UpgradeError::ReleaseLookup(description))
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
    /// over the running one.
    pub fn install(&self, release: &Release) -> Result<(), UpgradeError> {
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
        let staging = Staging::create()?;
        let archive = staging.path().join(&asset.name);
        download(release, asset, staging.path())?;
        if let Some(expected) = asset.digest.as_deref().and_then(sha256_hex) {
            let actual = hex(&Sha256::digest(fs::read(&archive)?));
            if actual != expected {
                return Err(UpgradeError::Checksum {
                    expected: expected.to_string(),
                    actual,
                });
            }
        }
        extract(staging.path(), &asset.name)?;
        let binary = staging.path().join(BIN_NAME);
        check_runs(&binary, release.version())?;
        replace(&self.exe, &binary)
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
    } else if cfg!(all(
        target_os = "linux",
        target_arch = "aarch64",
        target_env = "gnu"
    )) {
        Some("aarch64-unknown-linux-gnu")
    } else if cfg!(all(target_os = "windows", target_arch = "x86_64")) {
        supports_x86_64_v3().then_some("x86_64-pc-windows-msvc")
    } else {
        None
    }
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

fn download(release: &Release, asset: &Asset, dir: &Path) -> Result<(), UpgradeError> {
    let mut command = match fetcher().ok_or(UpgradeError::NoDownloader)? {
        Fetcher::Curl(curl) => {
            let mut command = Command::new(curl);
            command
                .args([
                    "--silent",
                    "--show-error",
                    "--fail",
                    "--location",
                    "--connect-timeout",
                    "10",
                    "--max-time",
                    DOWNLOAD_TIMEOUT_SECS,
                    "--header",
                    &format!("User-Agent: superline/{CURRENT_VERSION}"),
                    "--output",
                ])
                .arg(dir.join(&asset.name))
                .arg(&asset.browser_download_url);
            command
        }
        Fetcher::Gh(gh) => {
            let mut command = Command::new(gh);
            command
                .args(["release", "download", &release.tag, "--repo", REPO])
                .args(["--pattern", &asset.name, "--dir"])
                .arg(dir);
            command
        }
    };
    run(&mut command).map_err(UpgradeError::Download)
}

/// Runs `tar` from inside `dir` on the archive's bare name: GNU tar, which Git
/// for Windows puts on `PATH`, reads the `C:` of an absolute Windows path as a
/// remote host.
fn extract(dir: &Path, archive: &str) -> Result<(), UpgradeError> {
    run(Command::new("tar").current_dir(dir).args(["-xzf", archive]))
        .map_err(UpgradeError::Extract)?;
    if dir.join(BIN_NAME).is_file() {
        Ok(())
    } else {
        Err(UpgradeError::Extract(format!(
            "the archive has no {BIN_NAME}"
        )))
    }
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

/// Runs `command`, turning a failure into its stderr for the error message.
fn run(command: &mut Command) -> Result<(), String> {
    let output = command
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .output()
        .map_err(|error| error.to_string())?;
    if output.status.success() {
        return Ok(());
    }
    let stderr = String::from_utf8_lossy(&output.stderr);
    Err(match stderr.trim() {
        "" => format!("exited with {}", output.status),
        stderr => stderr.to_string(),
    })
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
        assert_eq!(release.assets[1].digest, None);

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
