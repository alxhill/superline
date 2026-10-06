use std::cmp::Ordering;
use std::fmt::Write;
use std::io::Read;
use std::marker::PhantomData;
use std::path::{Component, Path, PathBuf};
use std::time::Duration;
use std::{env, fs};

use serde::{Deserialize, Serialize};

use crate::cache::{hash_id, Cached, Lookup, Source};
use crate::colors::Color;
use crate::config::{GitBackend, SegmentPadding, DEFAULT_GIT_STATUS_TIMEOUT_MS};
use crate::debug;
use crate::themes::DefaultColors;
use crate::utils::join_non_empty;
use crate::{Powerline, Style};

use super::{DefaultPadding, Module};

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
    worktrees: bool,
    repo: bool,
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

    const DEFAULT_WORKTREE_ICON: &'static str = "\u{f1897}"; // nf-md-forest
    /// Shown before the linked-worktree label. Empty shows just the numbers.
    fn git_worktree_icon() -> &'static str {
        Self::DEFAULT_WORKTREE_ICON
    }

    const DEFAULT_BRANCH_ICON: &'static str = GIT_ICON;
    const DEFAULT_LINKED_WORKTREE_ICON: &'static str = LINKED_WORKTREE_ICON;
    const DEFAULT_DETACHED_ICON: &'static str = DETACHED_ARROW;
    const DEFAULT_REMOTE_ICON: &'static str = GITHUB_LOGO;
    const DEFAULT_AHEAD_ICON: &'static str = UP_ARROW;
    const DEFAULT_BEHIND_ICON: &'static str = DOWN_ARROW;

    fn git_notstaged_icon() -> &'static str {
        Self::NOT_STAGED_SYMBOL
    }
    fn git_staged_icon() -> &'static str {
        Self::STAGED_SYMBOL
    }
    fn git_untracked_icon() -> &'static str {
        Self::UNTRACKED_SYMBOL
    }
    fn git_conflicted_icon() -> &'static str {
        Self::CONFLICTED_SYMBOL
    }
    /// Before the branch name in a repository's main working tree.
    fn git_branch_icon() -> &'static str {
        Self::DEFAULT_BRANCH_ICON
    }
    /// Before the branch name in a linked worktree, in place of the branch
    /// icon.
    fn git_linked_worktree_icon() -> &'static str {
        Self::DEFAULT_LINKED_WORKTREE_ICON
    }
    /// Between a detached HEAD's hash and the branch it sits on.
    fn git_detached_icon() -> &'static str {
        Self::DEFAULT_DETACHED_ICON
    }
    fn git_remote_icon() -> &'static str {
        Self::DEFAULT_REMOTE_ICON
    }
    fn git_ahead_icon() -> &'static str {
        Self::DEFAULT_AHEAD_ICON
    }
    fn git_behind_icon() -> &'static str {
        Self::DEFAULT_BEHIND_ICON
    }
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
            true,
            true,
        )
    }

    /// `worktrees` shows the linked-worktree count next to the branch; `repo`
    /// shows the remote segment with its link and ahead/behind counts, the
    /// segment [`GitRemote`] draws on its own.
    pub fn with_config(
        status_timeout: Duration,
        backend: GitBackend,
        worktrees: bool,
        repo: bool,
    ) -> Git<S> {
        Git {
            status_timeout,
            backend,
            worktrees,
            repo,
            scheme: PhantomData,
        }
    }
}

/// The remote segment of [`Git`] as a widget of its own, so it can sit
/// anywhere in the prompt: the forge logo linking to the repository, and the
/// commits ahead of and behind the upstream. It draws from the same status
/// lookup as [`Git`], which a prompt makes once however many of the two draw
/// from it.
pub struct GitRemote<S> {
    status_timeout: Duration,
    backend: GitBackend,
    ahead_behind: bool,
    scheme: PhantomData<S>,
}

impl<S: GitScheme> Default for GitRemote<S> {
    fn default() -> Self {
        Self::new()
    }
}

impl<S: GitScheme> GitRemote<S> {
    pub fn new() -> GitRemote<S> {
        Self::with_config(
            Duration::from_millis(DEFAULT_GIT_STATUS_TIMEOUT_MS),
            GitBackend::default(),
            true,
        )
    }

    /// `status_timeout` and `backend` only matter when no [`Git`] widget has
    /// looked the status up first; `ahead_behind` adds the counts after the
    /// logo.
    pub fn with_config(
        status_timeout: Duration,
        backend: GitBackend,
        ahead_behind: bool,
    ) -> GitRemote<S> {
        GitRemote {
            status_timeout,
            backend,
            ahead_behind,
            scheme: PhantomData,
        }
    }
}

#[derive(Clone, Serialize, Deserialize)]
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
    /// Linked worktrees of the repository, from [`linked_worktrees`].
    #[serde(default)]
    pub worktrees: u32,
    /// This checkout's 1-based position among them in `git worktree list`
    /// order. `None` in the main checkout.
    #[serde(default)]
    pub worktree_index: Option<u32>,
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

/// The branch name as the prompt shows it. The cached name always marks a
/// detached HEAD with [`DETACHED_ARROW`] (see [`detached_label`]), so the
/// theme's icon is swapped in here.
fn branch_label(branch_name: &str, detached_icon: &str) -> String {
    match branch_name.split_once(&format!(" {DETACHED_ARROW} ")) {
        Some((hash, branch)) => join_non_empty([hash, detached_icon, branch]),
        None => branch_name.to_owned(),
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

/// The linked worktrees of a repository, as `git worktree list` shows them
/// after the main checkout, less the ones it marks prunable.
#[derive(Debug, Default, PartialEq)]
pub(super) struct Worktrees {
    pub count: u32,
    /// The 1-based position of the checkout asked about in that list. `None`
    /// in the main checkout.
    pub index: Option<u32>,
}

/// The linked worktrees of the repository checked out at `worktree` (its main
/// checkout or any linked one), and where `worktree` sits among them. Costs a
/// directory listing rather than a `git worktree list` spawn.
pub(super) fn linked_worktrees(worktree: &Path) -> Worktrees {
    let Some(git_dir) = resolve_git_dir(worktree) else {
        return Worktrees::default();
    };
    match linked_common_dir(&git_dir) {
        // The main checkout has no position, so it can skip the sort.
        None => Worktrees {
            count: listed_worktrees(&git_dir).len() as u32,
            index: None,
        },
        Some(common) => {
            let listed = in_list_order(listed_worktrees(&common), &common);
            Worktrees {
                count: listed.len() as u32,
                index: position_of(&git_dir, &listed),
            }
        }
    }
}

/// The git directory every worktree of a repository shares, when `git_dir`
/// belongs to a linked worktree: it names it in a `commondir` file, usually
/// `../..`. Any other git directory, a submodule's under the superproject's
/// `.git/modules/` included, is its own common directory.
fn linked_common_dir(git_dir: &Path) -> Option<PathBuf> {
    let common = fs::read_to_string(git_dir.join("commondir")).ok()?;
    Some(git_dir.join(common.trim()))
}

/// A linked worktree's entry under the common git directory's `worktrees/`.
struct LinkedWorktree {
    admin: PathBuf,
    /// The `gitdir` file: the checkout's `.git`, absolute or relative to
    /// `admin`.
    gitdir: String,
}

impl LinkedWorktree {
    /// The checkout path `git worktree list` prints and sorts by. Git keeps an
    /// absolute `gitdir` as written and resolves a relative one.
    fn listed_path(&self) -> PathBuf {
        let path = Path::new(self.gitdir.strip_suffix("/.git").unwrap_or(&self.gitdir));
        if path.is_absolute() {
            path.to_path_buf()
        } else {
            realpath_forgiving(&self.admin.join(path))
        }
    }
}

/// The entries under `common`'s `worktrees/` that `git worktree list` shows
/// without marking them prunable, in directory order. Git leaves out entries
/// without a `gitdir` file, and marks the ones whose checkout is gone
/// prunable unless they are locked.
fn listed_worktrees(common: &Path) -> Vec<LinkedWorktree> {
    let Ok(entries) = fs::read_dir(common.join("worktrees")) else {
        return Vec::new();
    };
    entries
        .flatten()
        .filter_map(|entry| {
            let admin = entry.path();
            let gitdir = fs::read_to_string(admin.join("gitdir")).ok()?;
            let gitdir = gitdir.trim();
            let live = admin.join("locked").exists() || admin.join(gitdir).exists();
            (!gitdir.is_empty() && live).then(|| LinkedWorktree {
                gitdir: gitdir.to_owned(),
                admin,
            })
        })
        .collect()
}

/// Sorts `listed` the way `git worktree list` does: by path, ASCII case
/// folded when the repository sets `core.ignorecase`.
fn in_list_order(mut listed: Vec<LinkedWorktree>, common: &Path) -> Vec<LinkedWorktree> {
    if listed.len() > 1 {
        let ignore_case = ignores_case(common);
        listed.sort_by_cached_key(|worktree| path_sort_key(&worktree.listed_path(), ignore_case));
    }
    listed
}

/// `core.ignorecase` from the repository's own config, where `git init` and
/// `git clone` record it on a case-insensitive filesystem.
fn ignores_case(common: &Path) -> bool {
    gix::config::File::from_path_no_includes(common.join("config"), gix::config::Source::Local)
        .ok()
        .and_then(|config| config.boolean("core.ignorecase"))
        .is_some_and(|value| value.unwrap_or(false))
}

/// The bytes git compares worktree paths by (`fspathcmp`), with a Windows
/// path spelled the way git writes it.
fn path_sort_key(path: &Path, ignore_case: bool) -> Vec<u8> {
    let mut key = path.as_os_str().as_encoded_bytes().to_vec();
    if cfg!(windows) {
        if key.starts_with(br"\\?\") {
            key.drain(..4);
        }
        key.iter_mut()
            .filter(|byte| **byte == b'\\')
            .for_each(|byte| *byte = b'/');
    }
    if ignore_case {
        key.make_ascii_lowercase();
    }
    key
}

/// Resolves `path` like git's `strbuf_realpath_forgiving`: symlinks as far as
/// the path exists, then the missing rest lexically.
fn realpath_forgiving(path: &Path) -> PathBuf {
    let components: Vec<Component> = path.components().collect();
    for existing in (1..=components.len()).rev() {
        let Ok(mut real) = fs::canonicalize(components[..existing].iter().collect::<PathBuf>())
        else {
            continue;
        };
        for component in &components[existing..] {
            match component {
                Component::ParentDir => {
                    real.pop();
                }
                Component::Normal(name) => real.push(name),
                _ => {}
            }
        }
        return real;
    }
    path.to_path_buf()
}

/// The 1-based position in `listed` of the linked worktree whose git
/// directory is `git_dir`. Compares resolved paths, so a relative or
/// symlinked `gitdir:` in the checkout's `.git` file still finds its entry.
fn position_of(git_dir: &Path, listed: &[LinkedWorktree]) -> Option<u32> {
    let current = fs::canonicalize(git_dir).ok()?;
    let position = listed.iter().position(|worktree| {
        fs::canonicalize(&worktree.admin).is_ok_and(|admin| admin == current)
    })?;
    Some(position as u32 + 1)
}

/// The worktree label appended to the branch label: `index/count` inside a
/// linked worktree and `count` in the main checkout, after `icon` unless it
/// is empty.
fn worktree_label(icon: &str, count: u32, index: Option<u32>) -> String {
    let position = match index {
        Some(index) => format!("{index}/{count}"),
        None => count.to_string(),
    };
    if icon.is_empty() {
        position
    } else {
        format!("{icon} {position}")
    }
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
pub(super) fn git_on_path() -> bool {
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
const LINKED_WORKTREE_ICON: &str = "\u{f1bb}";
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
    fn default_padding(&self) -> DefaultPadding {
        SegmentPadding::Large.into()
    }

    fn append_segments(&mut self, powerline: &mut Powerline) {
        let (git_dir, is_worktree) = match find_git_dir() {
            Some(result) => result,
            _ => return,
        };

        let icon = if is_worktree {
            S::git_linked_worktree_icon()
        } else {
            S::git_branch_icon()
        };
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

        let stats = match Cached::new(source).load_with_timeout_shared(self.status_timeout) {
            Lookup::Ready(stats) => stats,
            Lookup::Loading => {
                powerline.add_segment(
                    join_non_empty([icon, "loading…"]),
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

        let branch = branch_label(&stats.branch_name, S::git_detached_icon());
        let mut branch = join_non_empty([icon, branch.as_str()]);
        if self.worktrees && stats.worktrees > 0 {
            let label = worktree_label(
                S::git_worktree_icon(),
                stats.worktrees,
                stats.worktree_index,
            );
            let _ = write!(branch, " {label}");
        }
        powerline.add_segment(branch, Style::simple(branch_fg, branch_bg));

        let add_elem = |powerline: &mut Powerline, count: u32, symbol, fg, bg| match count.cmp(&1) {
            Ordering::Equal | Ordering::Greater => powerline.add_segment(
                join_non_empty([count.to_string().as_str(), symbol]),
                Style::simple(fg, bg),
            ),
            Ordering::Less => (),
        };

        add_elem(
            powerline,
            stats.non_staged,
            S::git_notstaged_icon(),
            S::git_notstaged_fg(),
            S::git_notstaged_bg(),
        );
        add_elem(
            powerline,
            stats.untracked,
            S::git_untracked_icon(),
            S::git_untracked_fg(),
            S::git_untracked_bg(),
        );
        add_elem(
            powerline,
            stats.staged,
            S::git_staged_icon(),
            S::git_staged_fg(),
            S::git_staged_bg(),
        );
        add_elem(
            powerline,
            stats.conflicted,
            S::git_conflicted_icon(),
            S::git_conflicted_fg(),
            S::git_conflicted_bg(),
        );

        if self.repo {
            append_remote::<S>(powerline, &stats, true);
        }
    }
}

impl<S: GitScheme> Module for GitRemote<S> {
    fn default_padding(&self) -> DefaultPadding {
        SegmentPadding::Large.into()
    }

    fn append_segments(&mut self, powerline: &mut Powerline) {
        let Some((git_dir, _)) = find_git_dir() else {
            return;
        };
        let source = GitStatus {
            git_dir,
            backend: self.backend,
        };
        // Nothing to show until the status is in: `git` says `loading…`.
        if let Lookup::Ready(stats) =
            Cached::new(source).load_with_timeout_shared(self.status_timeout)
        {
            append_remote::<S>(powerline, &stats, self.ahead_behind);
        }
    }
}

/// The remote segment, when the repository has a remote: the forge logo,
/// linking to the repository's page when it has one, then the ahead and
/// behind counts unless `ahead_behind` is off.
fn append_remote<S: GitScheme>(powerline: &mut Powerline, stats: &GitStats, ahead_behind: bool) {
    if !stats.remote {
        return;
    }
    let counts = |count: u32| if ahead_behind { count } else { 0 };
    let remote = remote_label(
        S::git_remote_icon(),
        (counts(stats.ahead), S::git_ahead_icon()),
        (counts(stats.behind), S::git_behind_icon()),
    );

    let style = Style::simple(S::git_remote_fg(), S::git_remote_bg());
    match &stats.remote_url {
        _ if remote.is_empty() => {}
        Some(url) => powerline.add_hyperlink_segment(&remote, url, style, None),
        None => powerline.add_segment(remote, style),
    }
}

/// The remote segment: the remote icon, then the ahead and behind counts each
/// followed by its icon.
fn remote_label(icon: &str, ahead: (u32, &str), behind: (u32, &str)) -> String {
    let mut counts = String::new();
    if ahead.0 > 0 {
        let _ = write!(counts, "{}{} ", ahead.0, ahead.1);
    }
    if behind.0 > 0 {
        let _ = write!(counts, "{}{}", behind.0, behind.1);
    }
    join_non_empty([icon, counts.as_str()])
}

#[cfg(test)]
mod tests {
    use std::io::Write;
    use std::path::Path;
    use std::sync::atomic::{AtomicU32, Ordering};

    use super::{
        branch_label, choose_backend, detached_label, in_list_order, index_entry_count,
        install_prefix_of, linked_common_dir, linked_worktrees, listed_worktrees, parse_head,
        preferred_branch, preferred_remote, prefers_cli_for_index_count, remote_label,
        remote_web_url, resolve_git_dir, worktree_label, Choice, GitBackend, GitStats, Reason,
        Worktrees, AUTO_CLI_ENTRY_THRESHOLD,
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
    fn branch_label_swaps_in_the_theme_detached_icon() {
        let detached = detached_label(Some("main".into()), "abc1234");
        assert_eq!(branch_label(&detached, "\u{f432}"), "abc1234 \u{f432} main");
        assert_eq!(branch_label(&detached, "->"), "abc1234 -> main");
        assert_eq!(branch_label(&detached, ""), "abc1234 main");
        assert_eq!(branch_label("ah/feature", ""), "ah/feature");
    }

    #[test]
    fn remote_label_hides_empty_icons_with_their_spaces() {
        assert_eq!(remote_label("R", (0, "^"), (0, "v")), "R");
        assert_eq!(remote_label("R", (2, "^"), (1, "v")), "R 2^ 1v");
        assert_eq!(remote_label("", (2, "^"), (1, "v")), "2^ 1v");
        assert_eq!(remote_label("", (0, "^"), (1, "")), "1");
        assert_eq!(remote_label("", (0, "^"), (0, "v")), "");
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

    fn scratch_dir() -> std::path::PathBuf {
        static COUNTER: AtomicU32 = AtomicU32::new(0);
        let n = COUNTER.fetch_add(1, Ordering::Relaxed);
        let dir =
            std::env::temp_dir().join(format!("superline-worktrees-{}-{n}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn git(dir: &Path, args: &[&str]) -> String {
        let output = std::process::Command::new("git")
            .current_dir(dir)
            .args(args)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "`git {}` failed: {}",
            args.join(" "),
            String::from_utf8_lossy(&output.stderr)
        );
        String::from_utf8(output.stdout).unwrap()
    }

    fn init_repo(dir: &Path) {
        std::fs::create_dir_all(dir).unwrap();
        git(dir, &["init", "-q", "-b", "main"]);
        git(dir, &["config", "user.email", "test@example.com"]);
        git(dir, &["config", "user.name", "test"]);
        git(dir, &["commit", "-q", "--allow-empty", "-m", "init"]);
    }

    fn add_worktree(repo: &Path, path: &Path, branch: &str) {
        git(
            repo,
            &[
                "worktree",
                "add",
                "-q",
                "-b",
                branch,
                path.to_str().unwrap(),
            ],
        );
    }

    /// What git itself reports from `checkout`: the linked worktrees `git
    /// worktree list --porcelain` shows after the main one without marking
    /// them prunable, as branch names in its order, and the 1-based position
    /// of `checkout` among them.
    fn listed_by_git(checkout: &Path) -> (Vec<String>, Option<u32>) {
        let listing = git(checkout, &["worktree", "list", "--porcelain"]);
        let here = std::fs::canonicalize(checkout).unwrap();
        let (mut branches, mut index) = (Vec::new(), None);
        let entries = listing
            .split("\n\n")
            .filter(|entry| entry.starts_with("worktree "));
        for entry in entries.skip(1) {
            if entry.lines().any(|line| line.starts_with("prunable")) {
                continue;
            }
            let branch = entry
                .lines()
                .find_map(|line| line.strip_prefix("branch refs/heads/"))
                .unwrap_or("(detached)");
            branches.push(branch.to_owned());
            let path = entry
                .lines()
                .next()
                .unwrap()
                .strip_prefix("worktree ")
                .unwrap();
            if std::fs::canonicalize(path).is_ok_and(|path| path == here) {
                index = Some(branches.len() as u32);
            }
        }
        (branches, index)
    }

    /// The branch names of the worktrees [`linked_worktrees`] counts from
    /// `checkout`, in the order it numbers them.
    fn listed_by_superline(checkout: &Path) -> Vec<String> {
        let git_dir = resolve_git_dir(checkout).unwrap();
        let common = linked_common_dir(&git_dir).unwrap_or(git_dir);
        in_list_order(listed_worktrees(&common), &common)
            .iter()
            .map(|worktree| {
                parse_head(&std::fs::read_to_string(worktree.admin.join("HEAD")).unwrap()).unwrap()
            })
            .collect()
    }

    /// Asserts that git, the helper and both backends agree from `checkout`:
    /// `count` linked worktrees in the same order, with `checkout` at `index`.
    fn assert_worktrees(checkout: &Path, count: u32, index: Option<u32>) {
        let (order, git_index) = listed_by_git(checkout);
        assert_eq!(
            (order.len() as u32, git_index),
            (count, index),
            "git from {checkout:?}: {order:?}"
        );
        assert_eq!(
            listed_by_superline(checkout),
            order,
            "order from {checkout:?}"
        );
        assert_eq!(
            linked_worktrees(checkout),
            Worktrees { count, index },
            "{checkout:?}"
        );
        for (backend, stats) in [
            ("cli", super::process::run_git(checkout)),
            ("gitoxide", super::gitoxide::run_git(checkout)),
        ] {
            assert_eq!(
                (stats.worktrees, stats.worktree_index),
                (count, index),
                "{backend} backend from {checkout:?}"
            );
        }
    }

    #[test]
    fn linked_worktrees_are_counted_and_numbered_from_every_checkout() {
        let root = scratch_dir();
        let main = root.join("main");
        init_repo(&main);
        assert_worktrees(&main, 0, None);

        // Git numbers by path, so `a/wt2` comes before `b/wt` even though
        // their entries under `.git/worktrees/` are `wt2` and `wt`.
        let later = root.join("b").join("wt");
        add_worktree(&main, &later, "later");
        assert_worktrees(&main, 1, None);
        assert_worktrees(&later, 1, Some(1));

        let earlier = root.join("a").join("wt2");
        add_worktree(&main, &earlier, "earlier");
        assert_worktrees(&main, 2, None);
        assert_worktrees(&earlier, 2, Some(1));
        assert_worktrees(&later, 2, Some(2));

        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn the_order_folds_case_only_under_core_ignorecase() {
        let root = scratch_dir();
        let main = root.join("main");
        init_repo(&main);
        let (upper, lower) = (root.join("Zed"), root.join("alpha"));
        add_worktree(&main, &upper, "upper");
        add_worktree(&main, &lower, "lower");

        git(&main, &["config", "core.ignorecase", "true"]);
        assert_worktrees(&lower, 2, Some(1));
        assert_worktrees(&upper, 2, Some(2));

        git(&main, &["config", "core.ignorecase", "false"]);
        assert_worktrees(&upper, 2, Some(1));
        assert_worktrees(&lower, 2, Some(2));

        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn deleted_worktrees_are_left_out_unless_locked() {
        let root = scratch_dir();
        let main = root.join("main");
        init_repo(&main);
        let names = ["a-gone", "b-locked", "c-kept", "d-removed"];
        let [gone, locked, kept, removed] = names.map(|name| root.join(name));
        for (path, name) in [&gone, &locked, &kept, &removed].into_iter().zip(names) {
            add_worktree(&main, path, name);
        }
        git(&main, &["worktree", "lock", locked.to_str().unwrap()]);
        assert_worktrees(&kept, 4, Some(3));

        std::fs::remove_dir_all(&gone).unwrap();
        std::fs::remove_dir_all(&locked).unwrap();
        git(&main, &["worktree", "remove", removed.to_str().unwrap()]);
        assert_worktrees(&main, 2, None);
        assert_worktrees(&kept, 2, Some(2));

        git(&main, &["worktree", "prune"]);
        assert_worktrees(&kept, 2, Some(2));

        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn relative_and_symlinked_gitdirs_find_their_entry() {
        let root = scratch_dir();
        let main = root.join("main");
        init_repo(&main);
        let (relative, absolute) = (root.join("c-relative"), root.join("d-absolute"));
        add_worktree(&main, &absolute, "d-absolute");
        // `--relative-paths` (git 2.48+) writes both links relative. Sorting
        // by the unresolved `gitdir`, `<admin>/../../../c-relative`, would put
        // it after `d-absolute`.
        let added = std::process::Command::new("git")
            .current_dir(&main)
            .args([
                "worktree",
                "add",
                "-q",
                "--relative-paths",
                "-b",
                "c-relative",
            ])
            .arg(&relative)
            .status()
            .unwrap()
            .success();
        if !added {
            add_worktree(&main, &relative, "c-relative");
        }
        // Any git follows a relative `gitdir:` in a checkout's `.git` file.
        std::fs::write(
            absolute.join(".git"),
            "gitdir: ../main/.git/worktrees/d-absolute\n",
        )
        .unwrap();

        assert_worktrees(&relative, 2, Some(1));
        assert_worktrees(&absolute, 2, Some(2));

        #[cfg(unix)]
        {
            let nested = root.join("links");
            std::fs::create_dir_all(&nested).unwrap();
            let link = nested.join("absolute");
            std::os::unix::fs::symlink(&absolute, &link).unwrap();
            assert_eq!(
                linked_worktrees(&link),
                Worktrees {
                    count: 2,
                    index: Some(2)
                }
            );
        }

        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn a_submodule_counts_its_own_worktrees() {
        let root = scratch_dir();
        let (superproject, library) = (root.join("super"), root.join("library"));
        init_repo(&superproject);
        init_repo(&library);
        add_worktree(&superproject, &root.join("super-wt"), "super-wt");
        // Without a remote, a relative URL resolves against the superproject.
        git(
            &superproject,
            &[
                "-c",
                "protocol.file.allow=always",
                "submodule",
                "add",
                "-q",
                "../library",
                "lib",
            ],
        );
        let submodule = superproject.join("lib");
        assert!(submodule.join(".git").is_file());
        assert_worktrees(&submodule, 0, None);

        let submodule_wt = root.join("lib-wt");
        add_worktree(&submodule, &submodule_wt, "lib-wt");
        assert_worktrees(&submodule, 1, None);
        assert_worktrees(&submodule_wt, 1, Some(1));
        assert_worktrees(&superproject, 1, None);

        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn outside_a_repository_there_are_no_worktrees() {
        let root = scratch_dir();
        assert_eq!(linked_worktrees(&root), Worktrees::default());
        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn the_label_numbers_a_linked_worktree_and_counts_from_the_main_checkout() {
        assert_eq!(worktree_label("\u{f1897}", 2, None), "\u{f1897} 2");
        assert_eq!(worktree_label("\u{f1897}", 15, Some(3)), "\u{f1897} 3/15");
        assert_eq!(worktree_label("", 2, None), "2");
        assert_eq!(worktree_label("", 2, Some(1)), "1/2");
    }

    #[test]
    fn stats_cached_without_a_worktree_index_still_load() {
        let stats: GitStats = serde_json::from_str(
            r#"{"untracked":0,"conflicted":0,"non_staged":0,"ahead":0,"behind":0,
                "staged":0,"remote":false,"branch_name":"main","worktrees":2}"#,
        )
        .unwrap();
        assert_eq!((stats.worktrees, stats.worktree_index), (2, None));
    }
}
