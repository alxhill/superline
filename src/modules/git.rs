use std::cmp::Ordering;
use std::fmt::Write;
use std::io::Read;
use std::marker::PhantomData;
use std::path::{Path, PathBuf};
use std::time::Duration;
use std::{env, fs};

use serde::{Deserialize, Serialize};

use crate::cache::{hash_id, Cached, Lookup, Source};
use crate::colors::Color;
use crate::config::{GitBackend, DEFAULT_GIT_STATUS_TIMEOUT_MS};
use crate::debug;
use crate::themes::DefaultColors;
use crate::{Powerline, Style};

use super::Module;

mod gitoxide;
mod process;

// Backend selection. Both backends always compile in and are chosen at
// runtime through the `git` module's `backend` config (default `auto`):
// `cli` shells out to the `git` binary, the only backend that honours git's
// untracked cache and fsmonitor, which wins by a wide margin on large working
// trees; `gitoxide` walks the repository in-process with pure Rust, which
// wins on small trees where a process spawn costs more than the walk itself.
// `auto` decides from the size of `.git/index` (see `choose_backend`).
// Each backend exposes a `run_git(&Path) -> GitStats`.

pub struct Git<S> {
    status_timeout: Duration,
    backend: GitBackend,
    scheme: PhantomData<S>,
}

pub trait GitScheme: DefaultColors {
    fn git_remote_bg() -> Color {
        Self::default_bg()
    }
    fn git_remote_fg() -> Color {
        Self::default_fg()
    }
    fn git_staged_bg() -> Color {
        Self::default_bg()
    }
    fn git_staged_fg() -> Color {
        Self::default_fg()
    }
    fn git_notstaged_bg() -> Color {
        Self::default_bg()
    }
    fn git_notstaged_fg() -> Color {
        Self::default_fg()
    }
    fn git_untracked_bg() -> Color {
        Self::default_bg()
    }
    fn git_untracked_fg() -> Color {
        Self::default_fg()
    }
    fn git_conflicted_bg() -> Color {
        Self::default_bg()
    }
    fn git_conflicted_fg() -> Color {
        Self::default_fg()
    }
    fn git_repo_clean_bg() -> Color {
        Self::default_bg()
    }
    fn git_repo_clean_fg() -> Color {
        Self::default_fg()
    }
    fn git_repo_dirty_bg() -> Color {
        Self::default_bg()
    }
    fn git_repo_dirty_fg() -> Color {
        Self::default_fg()
    }

    const NOT_STAGED_SYMBOL: &'static str = PENCIL;
    const STAGED_SYMBOL: &'static str = "+";
    const UNTRACKED_SYMBOL: &'static str = "?";
    const CONFLICTED_SYMBOL: &'static str = FANCY_STAR;
}

impl<S: GitScheme> Default for Git<S> {
    fn default() -> Self {
        Self::new()
    }
}

impl<S: GitScheme> Git<S> {
    pub fn new() -> Git<S> {
        Self::with_config(
            Duration::from_millis(DEFAULT_GIT_STATUS_TIMEOUT_MS),
            GitBackend::default(),
        )
    }

    pub fn with_config(status_timeout: Duration, backend: GitBackend) -> Git<S> {
        Git {
            status_timeout,
            backend,
            scheme: PhantomData,
        }
    }
}

#[derive(Serialize, Deserialize)]
pub struct GitStats {
    pub untracked: u32,
    pub conflicted: u32,
    pub non_staged: u32,
    pub ahead: u32,
    pub behind: u32,
    pub staged: u32,
    /// Whether the repository has any remote configured. Repo-level, not
    /// branch-level: it stays true on a branch that was never pushed, and on
    /// one whose remote-tracking ref has been pruned.
    pub remote: bool,
    /// Browser URL of the repository, derived from the fetch URL of `origin`
    /// (or the first remote when there is no `origin`). `None` when the remote
    /// is a local path or the URL is unparseable.
    #[serde(default)]
    pub remote_url: Option<String>,
    pub branch_name: String,
}

impl GitStats {
    pub fn is_dirty(&self) -> bool {
        (self.untracked + self.conflicted + self.staged + self.non_staged) > 0
    }
}

/// Label for a detached HEAD. A worktree checked out at a branch tip without
/// a branch of its own (`git worktree add --detach`, `git checkout origin/main`)
/// is what `git status` calls "HEAD detached at main"; it shows as
/// `<hash>  main`. Once HEAD moves off every branch tip only the hash remains.
fn detached_label(branch: Option<String>, hash: &str) -> String {
    match branch {
        Some(branch) => format!("{hash} {DETACHED_ARROW} {branch}"),
        None => hash.to_owned(),
    }
}

/// Picks the remote whose URL the GitHub logo links to: `origin` when it
/// exists, otherwise the first remote by name.
pub(super) fn preferred_remote<'a>(names: impl IntoIterator<Item = &'a str>) -> Option<&'a str> {
    names
        .into_iter()
        .min_by_key(|name| (*name != "origin", *name))
}

/// Turns a git remote URL into the page a browser opens for the repository:
/// scp-style (`git@github.com:owner/repo.git`) and `ssh://`/`git://` URLs
/// become `https://host/path`, `http(s)://` URLs keep their scheme, and the
/// `.git` suffix and any user info or SSH port are dropped. Local paths and
/// `file://` URLs have no web page and yield `None`.
pub(super) fn remote_web_url(remote: &str) -> Option<String> {
    let remote = remote.trim();
    if remote.is_empty() {
        return None;
    }

    let (scheme, rest) = match remote.split_once("://") {
        Some((scheme, rest)) => (scheme.to_ascii_lowercase(), rest),
        // scp-style `user@host:path`, but not a Windows drive (`C:\...`) or a
        // path that contains a slash before the colon.
        None => match remote.split_once(':') {
            Some((host, path))
                if !host.contains('/') && host.len() > 1 && !path.starts_with("//") =>
            {
                return build_web_url("https", host, path)
            }
            _ => return None,
        },
    };

    let (host, path) = rest.split_once('/').unwrap_or((rest, ""));
    match scheme.as_str() {
        "http" | "https" => build_web_url(&scheme, host, path),
        "ssh" | "git" | "git+ssh" | "ssh+git" => {
            // ssh URLs may carry a port which the web host never uses.
            let host = host.rsplit_once(':').map_or(host, |(host, _)| host);
            build_web_url("https", host, path)
        }
        _ => None,
    }
}

fn build_web_url(scheme: &str, host: &str, path: &str) -> Option<String> {
    let host = host.rsplit_once('@').map_or(host, |(_, host)| host);
    let path = path.trim_matches('/');
    let path = path
        .strip_suffix(".git")
        .unwrap_or(path)
        .trim_end_matches('/');
    if host.is_empty() || path.is_empty() {
        return None;
    }
    Some(format!("{scheme}://{host}/{path}"))
}

/// Picks the branch to name when several share HEAD's commit: `main` or
/// `master` first, then the alphabetically first name. Backends call this
/// with local branches before falling back to remote-tracking ones.
fn preferred_branch(names: impl IntoIterator<Item = String>) -> Option<String> {
    names
        .into_iter()
        .min_by_key(|name| (!matches!(name.as_str(), "main" | "master"), name.clone()))
}

/// Returns the git directory and whether it's a worktree
pub(super) fn find_git_dir() -> Option<(PathBuf, bool)> {
    let mut git_dir = env::current_dir().ok()?;
    loop {
        git_dir.push(".git");

        // Check if .git is a directory (normal repo)
        if git_dir.is_dir() {
            git_dir.pop();
            return Some((git_dir, false));
        }

        // Check if .git is a file (worktree - contains "gitdir: <path>")
        if git_dir.is_file() {
            git_dir.pop();
            return Some((git_dir, true));
        }

        git_dir.pop();

        if !git_dir.pop() {
            return None;
        }
    }
}

/// The checked-out branch of the repository rooted at `worktree`, read straight
/// from `HEAD`. Returns `"HEAD"` for a detached head, matching what
/// `git rev-parse --abbrev-ref HEAD` prints.
pub(super) fn head_branch(worktree: &Path) -> Option<String> {
    let git_dir = resolve_git_dir(worktree)?;
    parse_head(&fs::read_to_string(git_dir.join("HEAD")).ok()?)
}

/// `.git` is a directory in a normal clone and a `gitdir:` pointer file in a
/// linked worktree, where the real git directory (and `HEAD`, and `index`)
/// lives under the main repository's `.git/worktrees/<name>`.
fn resolve_git_dir(worktree: &Path) -> Option<PathBuf> {
    let dot_git = worktree.join(".git");
    if dot_git.is_dir() {
        return Some(dot_git);
    }

    let pointer = fs::read_to_string(&dot_git).ok()?;
    let target = PathBuf::from(pointer.trim().strip_prefix("gitdir:")?.trim());
    Some(if target.is_absolute() {
        target
    } else {
        worktree.join(target)
    })
}

/// Repos at or above this many index entries favour the CLI backend: measured
/// crossover where the CLI's untracked-cache win outweighs its process-spawn
/// cost (52ms CLI vs 120ms gitoxide on a 10k-file repo; 10ms gitoxide vs 39ms
/// CLI on superline's own ~200-file repo).
const AUTO_CLI_ENTRY_THRESHOLD: u32 = 1500;

/// Which backend will walk the status, and what decided it. The reason is only
/// ever read by the `SUPERLINE_DEBUG` report; [`Source::fetch`] just needs
/// `cli`.
#[derive(Clone, Copy, PartialEq, Debug)]
struct Choice {
    cli: bool,
    reason: Reason,
}

/// Why a [`Choice`] came out the way it did.
#[derive(Clone, Copy, PartialEq, Debug)]
enum Reason {
    /// The `backend` setting named this backend outright.
    Configured,
    /// `auto`, settled by the entry count in `.git/index` against
    /// [`AUTO_CLI_ENTRY_THRESHOLD`]. `None` when the count can't be read.
    IndexEntries(Option<u32>),
    /// `auto` would have taken the CLI on size, but no `git` is on `PATH`.
    NoGitOnPath(Option<u32>),
}

/// Resolves `backend` for the repository at `worktree`.
///
/// `auto` picks the CLI for large working trees, where git's untracked cache
/// and fsmonitor outweigh its process spawn, and gitoxide for small ones. The
/// index header is read before `PATH` is consulted, so a small tree - which
/// takes gitoxide either way - never pays for the lookup.
fn choose_backend(worktree: &Path, backend: GitBackend) -> Choice {
    let cli = |cli, reason| Choice { cli, reason };

    match backend {
        GitBackend::Cli => cli(true, Reason::Configured),
        GitBackend::Gitoxide => cli(false, Reason::Configured),
        GitBackend::Auto => {
            let count =
                resolve_git_dir(worktree).and_then(|dir| index_entry_count(&dir.join("index")));
            match (prefers_cli_for_index_count(count), git_on_path()) {
                (true, true) => cli(true, Reason::IndexEntries(count)),
                (true, false) => cli(false, Reason::NoGitOnPath(count)),
                (false, _) => cli(false, Reason::IndexEntries(count)),
            }
        }
    }
}

impl Choice {
    /// How the choice reads in the `SUPERLINE_DEBUG` report, e.g.
    /// `gitoxide (auto: 82 index entries, below the 1500 threshold)`.
    fn describe(self) -> String {
        let backend = if self.cli { "cli" } else { "gitoxide" };
        let why = match (self.reason, self.cli) {
            (Reason::Configured, _) => String::from("configured"),
            (Reason::IndexEntries(None), _) => String::from("auto: unreadable index entry count"),
            (Reason::IndexEntries(Some(count)), true) => format!(
                "auto: {count} index entries, at or above the {AUTO_CLI_ENTRY_THRESHOLD} threshold"
            ),
            (Reason::IndexEntries(Some(count)), _) => format!(
                "auto: {count} index entries, below the {AUTO_CLI_ENTRY_THRESHOLD} threshold"
            ),
            (Reason::NoGitOnPath(Some(count)), _) => {
                format!("auto: {count} index entries, but no git on PATH")
            }
            (Reason::NoGitOnPath(None), _) => {
                String::from("auto: unreadable index entry count, but no git on PATH")
            }
        };
        format!("{backend} ({why})")
    }
}

/// The threshold rule in isolation: at least [`AUTO_CLI_ENTRY_THRESHOLD`]
/// entries, or the count being unreadable, favours the CLI.
fn prefers_cli_for_index_count(count: Option<u32>) -> bool {
    count.is_none_or(|count| count >= AUTO_CLI_ENTRY_THRESHOLD)
}

/// The names `Command::new("git")` can actually launch: Windows tries the bare
/// name and then appends `.exe`, every other platform only has the bare name.
#[cfg(windows)]
const GIT_EXE_NAMES: &[&str] = &["git", "git.exe"];
#[cfg(not(windows))]
const GIT_EXE_NAMES: &[&str] = &["git"];

/// Whether a `git` executable is reachable through `PATH`.
fn git_on_path() -> bool {
    git_dir_on_path().is_some()
}

/// The first directory on `PATH` holding a `git` executable.
///
/// This scans `PATH` rather than running `git --version`, because a process
/// spawn costs around 30ms on Windows - more than the whole gitoxide status
/// walk - so asking `git` whether it exists would cost more than picking the
/// backend can save. Finding a name that turns out not to be runnable is
/// harmless: [`process::run_git`] falls back to gitoxide when `git` cannot be
/// executed.
fn git_dir_on_path() -> Option<PathBuf> {
    env::split_paths(&env::var_os("PATH")?)
        .filter(|dir| !dir.as_os_str().is_empty())
        .find(|dir| GIT_EXE_NAMES.iter().any(|name| dir.join(name).is_file()))
}

/// The git installation prefix that holds `etc/gitconfig`, derived from the
/// directory a `git` executable sits in.
///
/// Git for Windows puts `git.exe` in `<root>\cmd`, `<root>\bin` and
/// `<root>\mingw64\bin`, and keeps the installation's `etc\gitconfig` at
/// `<root>`. This reaches the same answer gitoxide derives from
/// `git --exec-path` (`<root>/mingw64/libexec/git-core`, cut before its
/// `libexec` component) without running git at all.
fn install_prefix_of(git_dir: &Path) -> Option<PathBuf> {
    let parent = git_dir.parent()?;
    match parent.file_name().and_then(|name| name.to_str()) {
        // `mingw64\bin` and its siblings sit one level deeper than `cmd`.
        Some("mingw64" | "mingw32" | "clangarm64" | "usr") => {
            parent.parent().map(Path::to_path_buf)
        }
        _ => Some(parent.to_path_buf()),
    }
}

/// Points gitoxide at the git installation's own `gitconfig` instead of
/// leaving it to go looking for the file itself.
///
/// gitoxide does read that file - it is where Git for Windows keeps
/// `core.autocrlf`, without which the status walk disagrees with `git status`
/// about every CRLF file - but the only way it can find the directory holding
/// it is by running `git --exec-path`, a child it spawns with
/// `CREATE_NO_WINDOW`, which costs around 55ms: several times the status walk
/// it precedes. `GIT_CONFIG_SYSTEM` names the file outright and is consulted
/// first, so setting it removes the probe while still loading the same file.
///
/// Only Windows is affected, being the platform where the lookup costs a
/// process spawn; elsewhere the prefix is simply `/`. An existing
/// `GIT_CONFIG_SYSTEM` is left alone, and nothing is set unless the derived
/// file really exists, so a `git` child process is never handed a path to a
/// file that isn't there. When the prefix cannot be derived, gitoxide falls
/// back to probing, exactly as it did before.
///
/// Sets a process-wide environment variable, so this has to be called before
/// any threads are started: from `main`, ahead of building the prompt.
pub fn preresolve_system_gitconfig() {
    if !cfg!(windows) || env::var_os("GIT_CONFIG_SYSTEM").is_some() {
        return;
    }

    let config = git_dir_on_path()
        .as_deref()
        .and_then(install_prefix_of)
        .map(|prefix| prefix.join("etc").join("gitconfig"))
        .filter(|config| config.is_file());

    if let Some(config) = config {
        env::set_var("GIT_CONFIG_SYSTEM", config);
    }
}

/// Reads the entry count straight out of the 12-byte `.git/index` header,
/// without loading the (potentially large) rest of the file: a 4-byte `DIRC`
/// signature, a 4-byte big-endian version, then a 4-byte big-endian entry
/// count.
fn index_entry_count(index_path: &Path) -> Option<u32> {
    let mut file = fs::File::open(index_path).ok()?;
    let mut header = [0u8; 12];
    file.read_exact(&mut header).ok()?;
    (&header[0..4] == b"DIRC").then(|| u32::from_be_bytes(header[8..12].try_into().unwrap()))
}

fn parse_head(contents: &str) -> Option<String> {
    let head = contents.trim();
    let Some(reference) = head.strip_prefix("ref:") else {
        // A detached head records the commit hash instead of a ref.
        return (!head.is_empty()).then(|| String::from("HEAD"));
    };
    let reference = reference.trim();
    Some(
        reference
            .strip_prefix("refs/heads/")
            .unwrap_or(reference)
            .to_string(),
    )
}

const UP_ARROW: &str = "\u{f062}";
const DOWN_ARROW: &str = "\u{f063}";
const PENCIL: &str = "\u{eae9}";
const FANCY_STAR: &str = "\u{273C}";

const GITHUB_LOGO: &str = "\u{e709}";
const GIT_ICON: &str = "\u{e0a0}";
const WORKTREE_ICON: &str = "\u{f1bb}";
const DETACHED_ARROW: &str = "\u{f432}";

/// Git status for one repository. The status walk is always attempted live
/// (see [`Git::with_config`]); the cache only stands in when it takes too
/// long, so a cached value is never considered fresh.
#[derive(Clone, Serialize, Deserialize)]
pub struct GitStatus {
    pub git_dir: PathBuf,
    /// Configured backend choice. Not part of [`Source::cache_id`]: switching
    /// it changes how the value is produced, not which repository it is for,
    /// so it must not invalidate an existing cache entry.
    pub backend: GitBackend,
}

impl Source for GitStatus {
    type Value = GitStats;
    const KIND: &'static str = "git";
    const TTL: Duration = Duration::ZERO;

    fn cache_id(&self) -> String {
        hash_id(&self.git_dir)
    }

    fn fetch(&self) -> Option<GitStats> {
        Some(if choose_backend(&self.git_dir, self.backend).cli {
            process::run_git(&self.git_dir)
        } else {
            gitoxide::run_git(&self.git_dir)
        })
    }
}

impl<S: GitScheme> Module for Git<S> {
    fn append_segments(&mut self, powerline: &mut Powerline) {
        let (git_dir, is_worktree) = match find_git_dir() {
            Some(result) => result,
            _ => return,
        };

        let icon = if is_worktree { WORKTREE_ICON } else { GIT_ICON };
        let source = GitStatus {
            git_dir,
            backend: self.backend,
        };

        // The walk itself happens off this thread (and sometimes in a detached
        // child), too late to report from. Resolving the choice here instead
        // costs an index-header read and a `PATH` scan, which is why it is
        // only done when a report is actually going to be printed. `fetch`
        // asks the same function with the same inputs.
        if debug::enabled() {
            debug::note(
                "backend",
                choose_backend(&source.git_dir, source.backend).describe(),
            );
        }

        let stats = match Cached::new(source).load_with_timeout(self.status_timeout) {
            Lookup::Ready(stats) => stats,
            Lookup::Loading => {
                powerline.add_segment(
                    format!("{icon} loading…"),
                    Style::simple(S::git_repo_clean_fg(), S::git_repo_clean_bg()),
                );
                return;
            }
            Lookup::Unavailable => return,
        };

        let (branch_fg, branch_bg) = if stats.is_dirty() {
            (S::git_repo_dirty_fg(), S::git_repo_dirty_bg())
        } else {
            (S::git_repo_clean_fg(), S::git_repo_clean_bg())
        };

        powerline.add_segment(
            format!("{} {}", icon, stats.branch_name),
            Style::simple(branch_fg, branch_bg),
        );

        let add_elem = |powerline: &mut Powerline, count: u32, symbol, fg, bg| match count.cmp(&1) {
            Ordering::Equal | Ordering::Greater => {
                powerline.add_segment(format!("{} {}", count, symbol), Style::simple(fg, bg))
            }
            Ordering::Less => (),
        };

        add_elem(
            powerline,
            stats.non_staged,
            S::NOT_STAGED_SYMBOL,
            S::git_notstaged_fg(),
            S::git_notstaged_bg(),
        );
        add_elem(
            powerline,
            stats.untracked,
            S::UNTRACKED_SYMBOL,
            S::git_untracked_fg(),
            S::git_untracked_bg(),
        );
        add_elem(
            powerline,
            stats.staged,
            S::STAGED_SYMBOL,
            S::git_staged_fg(),
            S::git_staged_bg(),
        );
        add_elem(
            powerline,
            stats.conflicted,
            S::CONFLICTED_SYMBOL,
            S::git_conflicted_fg(),
            S::git_conflicted_bg(),
        );

        if stats.remote {
            let logo_padding = if stats.ahead > 0 || stats.behind > 0 {
                " "
            } else {
                ""
            };
            let mut remote: String = format!("{}{}", GITHUB_LOGO, logo_padding);

            if stats.ahead > 0 {
                let _ = write!(remote, "{}{} ", stats.ahead, UP_ARROW);
            }
            if stats.behind > 0 {
                let _ = write!(remote, "{}{}", stats.behind, DOWN_ARROW);
            }

            let style = Style::simple(S::git_remote_fg(), S::git_remote_bg());
            match &stats.remote_url {
                Some(url) => powerline.add_hyperlink_segment(&remote, url, style, None),
                None => powerline.add_segment(remote, style),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::io::Write;
    use std::path::Path;
    use std::sync::atomic::{AtomicU32, Ordering};

    use super::{
        choose_backend, detached_label, index_entry_count, install_prefix_of, parse_head,
        preferred_branch, preferred_remote, prefers_cli_for_index_count, remote_web_url, Choice,
        GitBackend, Reason, AUTO_CLI_ENTRY_THRESHOLD,
    };

    fn names(list: &[&str]) -> Vec<String> {
        list.iter().map(ToString::to_string).collect()
    }

    fn temp_file(bytes: &[u8]) -> std::path::PathBuf {
        static COUNTER: AtomicU32 = AtomicU32::new(0);
        let n = COUNTER.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!("superline-index-{}-{n}", std::process::id()));
        let mut file = std::fs::File::create(&path).unwrap();
        file.write_all(bytes).unwrap();
        path
    }

    fn index_header(entry_count: u32) -> Vec<u8> {
        let mut header = Vec::with_capacity(12);
        header.extend_from_slice(b"DIRC");
        header.extend_from_slice(&2u32.to_be_bytes());
        header.extend_from_slice(&entry_count.to_be_bytes());
        header
    }

    #[test]
    fn index_entry_count_reads_the_header() {
        let path = temp_file(&index_header(4321));
        assert_eq!(index_entry_count(&path), Some(4321));
        std::fs::remove_file(&path).ok();
    }

    #[test]
    fn index_entry_count_rejects_a_short_file() {
        let path = temp_file(b"DIRC\0\0\0\x02");
        assert_eq!(index_entry_count(&path), None);
        std::fs::remove_file(&path).ok();
    }

    #[test]
    fn index_entry_count_rejects_a_bad_signature() {
        let mut bytes = index_header(10);
        bytes[0..4].copy_from_slice(b"NOPE");
        let path = temp_file(&bytes);
        assert_eq!(index_entry_count(&path), None);
        std::fs::remove_file(&path).ok();
    }

    #[test]
    fn index_entry_count_is_none_for_a_missing_file() {
        let path = std::env::temp_dir().join("superline-index-missing-does-not-exist");
        assert_eq!(index_entry_count(&path), None);
    }

    #[test]
    fn auto_selection_favours_cli_at_and_above_the_threshold() {
        assert!(!prefers_cli_for_index_count(Some(
            AUTO_CLI_ENTRY_THRESHOLD - 1
        )));
        assert!(prefers_cli_for_index_count(Some(AUTO_CLI_ENTRY_THRESHOLD)));
        assert!(prefers_cli_for_index_count(Some(
            AUTO_CLI_ENTRY_THRESHOLD + 1
        )));
    }

    #[test]
    fn auto_selection_favours_cli_when_the_count_is_unreadable() {
        assert!(prefers_cli_for_index_count(None));
    }

    /// Every branch of the choice names its backend first, then what settled
    /// it, so the debug report explains a slow render without a rebuild.
    #[test]
    fn each_choice_describes_its_backend_and_its_reason() {
        let describe = |cli, reason| Choice { cli, reason }.describe();

        assert_eq!(describe(true, Reason::Configured), "cli (configured)");
        assert_eq!(describe(false, Reason::Configured), "gitoxide (configured)");
        assert_eq!(
            describe(false, Reason::IndexEntries(Some(82))),
            "gitoxide (auto: 82 index entries, below the 1500 threshold)"
        );
        assert_eq!(
            describe(true, Reason::IndexEntries(Some(4000))),
            "cli (auto: 4000 index entries, at or above the 1500 threshold)"
        );
        assert_eq!(
            describe(true, Reason::IndexEntries(None)),
            "cli (auto: unreadable index entry count)"
        );
        assert_eq!(
            describe(false, Reason::NoGitOnPath(Some(4000))),
            "gitoxide (auto: 4000 index entries, but no git on PATH)"
        );
        assert_eq!(
            describe(false, Reason::NoGitOnPath(None)),
            "gitoxide (auto: unreadable index entry count, but no git on PATH)"
        );
    }

    /// An explicit `backend` setting is taken at face value, without the
    /// index read or the `PATH` scan that `auto` needs.
    #[test]
    fn a_configured_backend_needs_no_inspection() {
        let nowhere = Path::new("/superline-does-not-exist");

        assert_eq!(
            choose_backend(nowhere, GitBackend::Cli),
            Choice {
                cli: true,
                reason: Reason::Configured
            }
        );
        assert_eq!(
            choose_backend(nowhere, GitBackend::Gitoxide),
            Choice {
                cli: false,
                reason: Reason::Configured
            }
        );
    }

    #[test]
    fn install_prefix_is_the_root_of_a_git_for_windows_layout() {
        let root = Path::new("C:/Program Files/Git");
        for bin in [
            "cmd",
            "bin",
            "mingw64/bin",
            "mingw32/bin",
            "clangarm64/bin",
            "usr/bin",
        ] {
            assert_eq!(
                install_prefix_of(&root.join(bin)).as_deref(),
                Some(root),
                "{bin} should resolve to the installation root"
            );
        }
    }

    #[test]
    fn install_prefix_of_a_parentless_directory_is_none() {
        assert_eq!(install_prefix_of(Path::new("")), None);
    }

    #[test]
    fn detached_label_points_at_the_branch_when_known() {
        assert_eq!(
            detached_label(Some("main".into()), "abc1234"),
            "abc1234 \u{f432} main"
        );
        assert_eq!(detached_label(None, "abc1234"), "abc1234");
    }

    #[test]
    fn preferred_branch_favours_main_then_alphabetical() {
        assert_eq!(preferred_branch(names(&[])), None);
        assert_eq!(
            preferred_branch(names(&["zeta", "ah/feature", "main"])).as_deref(),
            Some("main")
        );
        assert_eq!(
            preferred_branch(names(&["zeta", "master", "ah/feature"])).as_deref(),
            Some("master")
        );
        assert_eq!(
            preferred_branch(names(&["zeta", "ah/feature"])).as_deref(),
            Some("ah/feature")
        );
    }

    #[test]
    fn preferred_remote_favours_origin_then_alphabetical() {
        assert_eq!(preferred_remote([]), None);
        assert_eq!(preferred_remote(["upstream", "origin"]), Some("origin"));
        assert_eq!(preferred_remote(["upstream", "fork"]), Some("fork"));
    }

    #[test]
    fn scp_style_remotes_become_https_pages() {
        assert_eq!(
            remote_web_url("git@github.com:alxhill/superline.git").as_deref(),
            Some("https://github.com/alxhill/superline")
        );
        assert_eq!(
            remote_web_url("gitlab.com:group/sub/repo").as_deref(),
            Some("https://gitlab.com/group/sub/repo")
        );
    }

    #[test]
    fn ssh_and_git_remotes_drop_user_and_port() {
        assert_eq!(
            remote_web_url("ssh://git@github.com:22/alxhill/superline.git").as_deref(),
            Some("https://github.com/alxhill/superline")
        );
        assert_eq!(
            remote_web_url("git://github.com/alxhill/superline.git").as_deref(),
            Some("https://github.com/alxhill/superline")
        );
    }

    #[test]
    fn http_remotes_keep_their_scheme_and_lose_the_git_suffix() {
        assert_eq!(
            remote_web_url("https://github.com/alxhill/superline.git").as_deref(),
            Some("https://github.com/alxhill/superline")
        );
        assert_eq!(
            remote_web_url("https://user:token@github.com/alxhill/superline/").as_deref(),
            Some("https://github.com/alxhill/superline")
        );
        assert_eq!(
            remote_web_url("http://git.internal:8080/team/repo.git").as_deref(),
            Some("http://git.internal:8080/team/repo")
        );
    }

    #[test]
    fn local_remotes_have_no_web_page() {
        assert_eq!(remote_web_url("/srv/git/repo.git"), None);
        assert_eq!(remote_web_url("../other-repo"), None);
        assert_eq!(remote_web_url("file:///srv/git/repo.git"), None);
        assert_eq!(remote_web_url("C:\\repos\\project"), None);
        assert_eq!(remote_web_url(""), None);
        assert_eq!(remote_web_url("https://github.com"), None);
    }

    #[test]
    fn head_on_a_branch_yields_the_short_name() {
        assert_eq!(
            parse_head("ref: refs/heads/main\n").as_deref(),
            Some("main")
        );
        assert_eq!(
            parse_head("ref: refs/heads/ah/perf-fix\n").as_deref(),
            Some("ah/perf-fix")
        );
    }

    #[test]
    fn detached_head_reports_the_placeholder_name() {
        assert_eq!(
            parse_head("9c1a1802f3d4c5b6a7980123456789abcdef0123\n").as_deref(),
            Some("HEAD")
        );
        assert_eq!(parse_head("  \n"), None);
    }

    #[test]
    fn a_ref_outside_refs_heads_keeps_its_full_name() {
        assert_eq!(
            parse_head("ref: refs/remotes/origin/main\n").as_deref(),
            Some("refs/remotes/origin/main")
        );
    }
}
