use std::marker::PhantomData;
use std::path::PathBuf;
use std::process::Command;
use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::cache::{hash_id, Cached, Lookup, Source};
use crate::claude_code::ClaudeCodeStatus;
use crate::colors::{self, Color};
use crate::config::{SegmentPadding, DEFAULT_GIT_STATUS_TIMEOUT_MS};
use crate::themes::DefaultColors;
use crate::{Powerline, Style};

use super::{DefaultPadding, Module};

pub trait DiffScheme: DefaultColors {
    const DIFF_ADDED_FG: Color = colors::green();
    const DIFF_REMOVED_FG: Color = colors::red();

    fn diff_added_fg() -> Color {
        Self::DIFF_ADDED_FG
    }
    fn diff_removed_fg() -> Color {
        Self::DIFF_REMOVED_FG
    }
    fn diff_bg() -> Color {
        Self::default_bg()
    }
}

/// Lines added and removed: in Claude Code's status line the session's own
/// counts, in a shell prompt the working tree's uncommitted changes against
/// `HEAD`.
pub struct Diff<'a, S> {
    claude: Option<&'a ClaudeCodeStatus>,
    scheme: PhantomData<S>,
}

impl<'a, S: DiffScheme> Diff<'a, S> {
    pub fn new(claude: Option<&'a ClaudeCodeStatus>) -> Self {
        Diff {
            claude,
            scheme: PhantomData,
        }
    }
}

impl<S: DiffScheme> Module for Diff<'_, S> {
    fn default_padding(&self) -> DefaultPadding {
        SegmentPadding::Large.into()
    }

    fn append_segments(&mut self, powerline: &mut Powerline) {
        let stat = match self.claude {
            Some(status) => DiffStat {
                added: status.cost.total_lines_added.unwrap_or(0),
                removed: status.cost.total_lines_removed.unwrap_or(0),
            },
            None => match working_tree_diff() {
                Some(stat) => stat,
                None => return,
            },
        };
        if stat.added == 0 && stat.removed == 0 {
            return;
        }
        powerline.add_two_tone_segment(
            &format!("+{}", stat.added),
            &format!("-{}", stat.removed),
            S::diff_removed_fg(),
            Style::simple(S::diff_added_fg(), S::diff_bg()),
        );
    }
}

fn working_tree_diff() -> Option<DiffStat> {
    let (root, _) = super::git::find_git_dir()?;
    let timeout = Duration::from_millis(DEFAULT_GIT_STATUS_TIMEOUT_MS);
    match Cached::new(GitDiff { root }).load_with_timeout(timeout) {
        Lookup::Ready(stat) => Some(stat),
        Lookup::Loading | Lookup::Unavailable => None,
    }
}

#[derive(Serialize, Deserialize, Default, Debug, PartialEq)]
pub struct DiffStat {
    added: u64,
    removed: u64,
}

/// The uncommitted changes, staged or not, of the work tree at `root`.
#[derive(Clone, Serialize, Deserialize)]
pub struct GitDiff {
    root: PathBuf,
}

impl Source for GitDiff {
    type Value = DiffStat;
    const KIND: &'static str = "diff";
    const TTL: Duration = Duration::ZERO;

    fn cache_id(&self) -> String {
        hash_id(&self.root)
    }

    fn fetchable(&self) -> bool {
        super::git::git_on_path()
    }

    fn fetch(&self) -> Option<DiffStat> {
        let output = Command::new("git")
            .current_dir(&self.root)
            .env("GIT_OPTIONAL_LOCKS", "0")
            .args(["diff", "HEAD", "--numstat", "--no-ext-diff", "--no-renames"])
            .output()
            .ok()?;
        // An unborn HEAD has nothing to diff against yet.
        if !output.status.success() {
            return Some(DiffStat::default());
        }
        Some(parse_numstat(&String::from_utf8_lossy(&output.stdout)))
    }
}

/// Sums `git diff --numstat` output. Binary files report `-` for both counts
/// and are left out.
fn parse_numstat(numstat: &str) -> DiffStat {
    let mut stat = DiffStat::default();
    for line in numstat.lines() {
        let mut fields = line.split('\t');
        let (Some(Ok(added)), Some(Ok(removed))) = (
            fields.next().map(str::parse::<u64>),
            fields.next().map(str::parse::<u64>),
        ) else {
            continue;
        };
        stat.added += added;
        stat.removed += removed;
    }
    stat
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn numstat_sums_text_files_and_skips_binary_ones() {
        let numstat = "12\t3\tsrc/main.rs\n-\t-\tlogo.png\n0\t7\tREADME.md\n";
        assert_eq!(
            parse_numstat(numstat),
            DiffStat {
                added: 12,
                removed: 10
            }
        );
        assert_eq!(parse_numstat(""), DiffStat::default());
    }
}
