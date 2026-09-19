use std::cmp::Ordering;
use std::env;
use std::fmt::Write;
use std::marker::PhantomData;
use std::path::PathBuf;
use std::time::Duration;

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
use crate::{Segments, Style};

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

/// Returns the git directory and whether it's a worktree
fn find_git_dir() -> Option<(PathBuf, bool)> {
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
    fn append_segments(&mut self, segments: &mut Segments) {
        let (git_dir, is_worktree) = match find_git_dir() {
            Some(result) => result,
            _ => return,
        };

        let icon = if is_worktree { WORKTREE_ICON } else { GIT_ICON };
        let stats = match Cached::new(GitStatus { git_dir }).load_with_timeout(self.status_timeout)
        {
            Lookup::Ready(stats) => stats,
            Lookup::Loading => {
                segments.add_segment(
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

        segments.add_segment(
            format!("{} {}", icon, stats.branch_name),
            Style::simple(branch_fg, branch_bg),
        );

        let add_elem = |segments: &mut Segments, count: u32, symbol, fg, bg| match count.cmp(&1) {
            Ordering::Equal | Ordering::Greater => {
                segments.add_segment(format!("{} {}", count, symbol), Style::simple(fg, bg))
            }
            Ordering::Less => (),
        };

        add_elem(
            segments,
            stats.non_staged,
            S::NOT_STAGED_SYMBOL,
            S::git_notstaged_fg(),
            S::git_notstaged_bg(),
        );
        add_elem(
            segments,
            stats.untracked,
            S::UNTRACKED_SYMBOL,
            S::git_untracked_fg(),
            S::git_untracked_bg(),
        );
        add_elem(
            segments,
            stats.staged,
            S::STAGED_SYMBOL,
            S::git_staged_fg(),
            S::git_staged_bg(),
        );
        add_elem(
            segments,
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

            segments.add_segment(
                remote,
                Style::simple(S::git_remote_fg(), S::git_remote_bg()),
            );
        }
    }
}
