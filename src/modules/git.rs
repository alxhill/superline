use std::cmp::Ordering;
use std::collections::hash_map::DefaultHasher;
use std::env;
use std::fmt::Write;
use std::fs::{self, File};
use std::hash::{Hash, Hasher};
use std::io::Write as _;
use std::marker::PhantomData;
use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::thread;
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

use crate::colors::Color;
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

const RENDER_TIMEOUT: Duration = Duration::from_secs(1);

fn run_git_with_timeout<F>(
    git_dir: PathBuf,
    cache_path: Option<PathBuf>,
    timeout: Duration,
    run_git: F,
) -> Option<GitStats>
where
    F: FnOnce(&Path) -> GitStats + Send + 'static,
{
    let (sender, receiver) = mpsc::sync_channel(1);
    thread::spawn(move || {
        let stats = run_git(&git_dir);
        let _ = sender.send(stats);
    });

    match receiver.recv_timeout(timeout) {
        Ok(stats) => {
            if let Some(path) = cache_path {
                write_cache(&path, &stats);
            }
            Some(stats)
        }
        Err(_) => cache_path.as_deref().and_then(read_cache),
    }
}

fn cache_path_for(repo_root: &Path) -> Option<PathBuf> {
    let base = crate::platform::cache_dir()?;
    let mut hasher = DefaultHasher::new();
    repo_root.hash(&mut hasher);

    Some(
        base.join("superline")
            .join(format!("git-{:016x}.json", hasher.finish())),
    )
}

fn read_cache(path: &Path) -> Option<GitStats> {
    serde_json::from_reader(File::open(path).ok()?).ok()
}

fn write_cache(path: &Path, stats: &GitStats) {
    if let Some(parent) = path.parent() {
        let _ = fs::create_dir_all(parent);
    }

    let tmp = path.with_extension("tmp");
    if let Ok(mut file) = File::create(&tmp) {
        if serde_json::to_writer(&mut file, stats).is_ok() && file.flush().is_ok() {
            let _ = fs::rename(&tmp, path);
        }
    }
}

impl<S: GitScheme> Module for Git<S> {
    fn append_segments(&mut self, powerline: &mut Powerline) {
        let (git_dir, is_worktree) = match find_git_dir() {
            Some(result) => result,
            _ => return,
        };

        let cache_path = cache_path_for(&git_dir);
        let Some(stats) =
            run_git_with_timeout(git_dir, cache_path, RENDER_TIMEOUT, internal::run_git)
        else {
            return;
        };

        let (branch_fg, branch_bg) = if stats.is_dirty() {
            (S::git_repo_dirty_fg(), S::git_repo_dirty_bg())
        } else {
            (S::git_repo_clean_fg(), S::git_repo_clean_bg())
        };

        let icon = if is_worktree { WORKTREE_ICON } else { GIT_ICON };
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
    use std::fs;
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicU32, Ordering};
    use std::thread;
    use std::time::Duration;

    use super::{read_cache, run_git_with_timeout, write_cache, GitStats, RENDER_TIMEOUT};

    fn unique_temp_dir() -> PathBuf {
        static COUNTER: AtomicU32 = AtomicU32::new(0);
        let n = COUNTER.fetch_add(1, Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!("superline-git-{}-{n}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn stats(branch_name: &str) -> GitStats {
        GitStats {
            untracked: 0,
            conflicted: 0,
            non_staged: 0,
            ahead: 0,
            behind: 0,
            staged: 0,
            remote: false,
            branch_name: branch_name.to_owned(),
        }
    }

    #[test]
    fn git_render_timeout_is_one_second() {
        assert_eq!(RENDER_TIMEOUT, Duration::from_secs(1));
    }

    #[test]
    fn fresh_git_stats_are_returned_and_cached() {
        let dir = unique_temp_dir();
        let cache = dir.join("git.json");
        write_cache(&cache, &stats("stale"));

        let result = run_git_with_timeout(
            PathBuf::new(),
            Some(cache.clone()),
            Duration::from_secs(1),
            |_| stats("fresh"),
        );

        assert_eq!(result.unwrap().branch_name, "fresh");
        assert_eq!(read_cache(&cache).unwrap().branch_name, "fresh");

        fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn cached_git_stats_are_returned_when_rendering_times_out() {
        let dir = unique_temp_dir();
        let cache = dir.join("git.json");
        write_cache(&cache, &stats("cached"));

        let result = run_git_with_timeout(
            PathBuf::new(),
            Some(cache),
            Duration::from_millis(10),
            |_| {
                thread::sleep(Duration::from_millis(100));
                stats("fresh")
            },
        );

        assert_eq!(result.unwrap().branch_name, "cached");

        fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn git_module_is_skipped_when_rendering_times_out_without_a_cache() {
        let result = run_git_with_timeout(PathBuf::new(), None, Duration::from_millis(10), |_| {
            thread::sleep(Duration::from_millis(100));
            stats("fresh")
        });

        assert!(result.is_none());
    }
}
