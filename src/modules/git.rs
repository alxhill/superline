use std::cmp::Ordering;
use std::fmt::Write;
use std::marker::PhantomData;
use std::path::{Path, PathBuf};
use std::time::Duration;
use std::{env, fs};

use serde::{Deserialize, Serialize};

// Backend selection. At most one of these modules is compiled in; when more
// than one feature is enabled the precedence is `gitoxide` > `libgit` > the
// `git` CLI fallback. Each backend exposes a `run_git(&Path) -> GitStats`.
#[cfg(feature = "gitoxide")]
use gitoxide as internal;
#[cfg(all(feature = "libgit", not(feature = "gitoxide")))]
use libgit as internal;
#[cfg(not(any(feature = "libgit", feature = "gitoxide")))]
use process as internal;

use crate::cache::{hash_id, Cached, Lookup, Source};
use crate::colors::Color;
use crate::config::DEFAULT_GIT_STATUS_TIMEOUT_MS;
use crate::themes::DefaultColors;
use crate::{Powerline, Style};

use super::Module;

#[cfg(not(any(feature = "libgit", feature = "gitoxide")))]
mod process;

#[cfg(all(feature = "libgit", not(feature = "gitoxide")))]
mod libgit;

#[cfg(feature = "gitoxide")]
mod gitoxide;

pub struct Git<S> {
    status_timeout: Duration,
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
        Self::with_status_timeout(Duration::from_millis(DEFAULT_GIT_STATUS_TIMEOUT_MS))
    }

    pub fn with_status_timeout(status_timeout: Duration) -> Git<S> {
        Git {
            status_timeout,
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
/// `main@<hash>`. Once HEAD moves off every branch tip only the hash remains.
fn detached_label(branch: Option<String>, hash: &str) -> String {
    match branch {
        Some(branch) => format!("{branch}@{hash}"),
        None => hash.to_owned(),
    }
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
pub(super) fn head_branch(worktree: &Path, is_worktree: bool) -> Option<String> {
    let git_dir = resolve_git_dir(worktree, is_worktree)?;
    parse_head(&fs::read_to_string(git_dir.join("HEAD")).ok()?)
}

/// `.git` is a directory in a normal clone and a `gitdir:` pointer file in a
/// linked worktree, where `HEAD` lives under the main repository's
/// `.git/worktrees/<name>`.
fn resolve_git_dir(worktree: &Path, is_worktree: bool) -> Option<PathBuf> {
    let dot_git = worktree.join(".git");
    if !is_worktree {
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

/// Git status for one repository. The status walk is always attempted live
/// (see [`Git::with_status_timeout`]); the cache only stands in when it takes
/// too long, so a cached value is never considered fresh.
#[derive(Clone, Serialize, Deserialize)]
pub struct GitStatus {
    pub git_dir: PathBuf,
}

impl Source for GitStatus {
    type Value = GitStats;
    const KIND: &'static str = "git";
    const TTL: Duration = Duration::ZERO;

    fn cache_id(&self) -> String {
        hash_id(&self.git_dir)
    }

    fn fetch(&self) -> Option<GitStats> {
        Some(internal::run_git(&self.git_dir))
    }
}

impl<S: GitScheme> Module for Git<S> {
    fn append_segments(&mut self, powerline: &mut Powerline) {
        let (git_dir, is_worktree) = match find_git_dir() {
            Some(result) => result,
            _ => return,
        };

        let icon = if is_worktree { WORKTREE_ICON } else { GIT_ICON };
        let stats = match Cached::new(GitStatus { git_dir }).load_with_timeout(self.status_timeout)
        {
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

            powerline.add_segment(
                remote,
                Style::simple(S::git_remote_fg(), S::git_remote_bg()),
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{detached_label, parse_head, preferred_branch};

    fn names(list: &[&str]) -> Vec<String> {
        list.iter().map(ToString::to_string).collect()
    }

    #[test]
    fn detached_label_prefixes_the_branch_when_known() {
        assert_eq!(
            detached_label(Some("main".into()), "abc1234"),
            "main@abc1234"
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
