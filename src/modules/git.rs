use std::cmp::Ordering;
use std::fmt::Write;
use std::marker::PhantomData;
use std::path::{Path, PathBuf};

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

use crate::colors::Color;
use crate::metadata::FetchState;
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
    state: FetchState<GitStats>,
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
        Git {
            state: FetchState::Pending,
            scheme: PhantomData,
        }
    }

    pub fn with_state(state: FetchState<GitStats>) -> Git<S> {
        Git {
            state,
            scheme: PhantomData,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
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
    /// Whether this repository was entered through a linked worktree. Defaults
    /// for compatibility with older snapshots.
    #[serde(default)]
    pub worktree: bool,
}

impl GitStats {
    pub fn is_dirty(&self) -> bool {
        (self.untracked + self.conflicted + self.staged + self.non_staged) > 0
    }
}

/// Finds the git directory for `start`, returning the repository root and
/// whether `.git` is a file (a linked worktree) rather than a directory.
pub(crate) fn find_git_dir_from(start: &Path) -> Option<(PathBuf, bool)> {
    let mut dir = start.to_path_buf();
    loop {
        let git = dir.join(".git");
        if git.is_dir() {
            return Some((dir, false));
        }
        if git.is_file() {
            return Some((dir, true));
        }
        if !dir.pop() {
            return None;
        }
    }
}

/// Runs the configured git backend and stamps the worktree flag onto the
/// result. Invoked by the metadata server in a background thread.
pub(crate) fn fetch_git(repo_root: &Path, is_worktree: bool) -> GitStats {
    let mut stats = internal::run_git(repo_root);
    stats.worktree = is_worktree;
    stats
}

const UP_ARROW: &str = "\u{f062}";
const DOWN_ARROW: &str = "\u{f063}";
const PENCIL: &str = "\u{eae9}";
const FANCY_STAR: &str = "\u{273C}";

const GITHUB_LOGO: &str = "\u{e709}";
const GIT_ICON: &str = "\u{e0a0}";
const WORKTREE_ICON: &str = "\u{f1bb}";

impl<S: GitScheme> Module for Git<S> {
    fn append_segments(&mut self, powerline: &mut Powerline) {
        match &self.state {
            FetchState::Ready(stats) => {
                let icon = if stats.worktree {
                    WORKTREE_ICON
                } else {
                    GIT_ICON
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

                let add_elem =
                    |powerline: &mut Powerline, count: u32, symbol, fg, bg| match count.cmp(&1) {
                        Ordering::Equal | Ordering::Greater => powerline
                            .add_segment(format!("{} {}", count, symbol), Style::simple(fg, bg)),
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
            FetchState::Pending => {
                // Without a reading we don't know whether this is a worktree yet,
                // so use the generic git glyph while the server fetches.
                powerline.add_segment(
                    format!("{GIT_ICON} loading…"),
                    Style::simple(S::git_repo_clean_fg(), S::git_repo_clean_bg()),
                );
            }
            FetchState::Unavailable => {}
        }
    }
}
