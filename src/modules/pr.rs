use std::marker::PhantomData;
use std::path::{Path, PathBuf};
use std::process::Command;

use serde::{Deserialize, Serialize};

use crate::colors::Color;
use crate::metadata::FetchState;
use crate::themes::DefaultColors;
use crate::{Powerline, Style};

use super::Module;

/// Branches that never have a PR of their own - skip all work for these.
pub(crate) const SKIP_BRANCHES: &[&str] = &["develop", "main", "master", "HEAD"];

pub struct Pr<S> {
    /// Whether to append the CI check-status dot after the PR number.
    show_status: bool,
    state: FetchState<Option<PrInfo>>,
    scheme: PhantomData<S>,
}

pub trait PrScheme: DefaultColors {
    fn pr_draft_fg() -> Color {
        Self::default_fg()
    }
    fn pr_draft_bg() -> Color {
        Self::default_bg()
    }
    fn pr_open_fg() -> Color {
        Self::default_fg()
    }
    fn pr_open_bg() -> Color {
        Self::default_bg()
    }
    fn pr_merged_fg() -> Color {
        Self::default_fg()
    }
    fn pr_merged_bg() -> Color {
        Self::default_bg()
    }
    fn pr_closed_fg() -> Color {
        Self::default_fg()
    }
    fn pr_closed_bg() -> Color {
        Self::default_bg()
    }
    fn pr_icon() -> &'static str {
        "\u{ea64}" // nf-cod-git_pull_request
    }

    fn pr_status_success_fg() -> Color {
        Self::default_fg()
    }
    fn pr_status_failure_fg() -> Color {
        Self::default_fg()
    }
    fn pr_status_pending_fg() -> Color {
        Self::default_fg()
    }
    fn pr_status_icon() -> &'static str {
        "\u{25cf}" // ● black circle
    }
}

impl<S: PrScheme> Default for Pr<S> {
    fn default() -> Self {
        Self::new(true)
    }
}

impl<S: PrScheme> Pr<S> {
    pub fn new(show_status: bool) -> Pr<S> {
        Pr {
            show_status,
            state: FetchState::Pending,
            scheme: PhantomData,
        }
    }

    pub fn with_state(show_status: bool, state: FetchState<Option<PrInfo>>) -> Pr<S> {
        Pr {
            show_status,
            state,
            scheme: PhantomData,
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum PrState {
    Draft,
    Open,
    Merged,
    Closed,
}

impl PrState {
    /// Whether the PR is still in progress (open or draft, but not merged or
    /// closed). Only these PRs show the CI status indicator.
    fn is_open(self) -> bool {
        matches!(self, PrState::Open | PrState::Draft)
    }

    /// Picks the (fg, bg) colors for this state from the active scheme.
    fn style<S: PrScheme>(self) -> (Color, Color) {
        match self {
            PrState::Draft => (S::pr_draft_fg(), S::pr_draft_bg()),
            PrState::Open => (S::pr_open_fg(), S::pr_open_bg()),
            PrState::Merged => (S::pr_merged_fg(), S::pr_merged_bg()),
            PrState::Closed => (S::pr_closed_fg(), S::pr_closed_bg()),
        }
    }
}

/// Aggregate state of the PR's checks, collapsed from the individual check runs
/// and status contexts reported by GitHub.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum CheckStatus {
    Success,
    Failure,
    Pending,
}

impl CheckStatus {
    /// Picks the dot's foreground colour from the active scheme. The dot shares
    /// the PR segment's background, so there's no background to choose here.
    fn fg<S: PrScheme>(self) -> Color {
        match self {
            CheckStatus::Success => S::pr_status_success_fg(),
            CheckStatus::Failure => S::pr_status_failure_fg(),
            CheckStatus::Pending => S::pr_status_pending_fg(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct PrInfo {
    pub number: u64,
    pub url: String,
    pub state: PrState,
    /// Aggregate CI status. `None` means there are no meaningful checks, so the
    /// dot is hidden rather than shown misleadingly. Defaulted for forward
    /// compatibility with snapshots written before this field existed.
    #[serde(default)]
    pub checks: Option<CheckStatus>,
}

impl<S: PrScheme> Module for Pr<S> {
    fn append_segments(&mut self, powerline: &mut Powerline) {
        let FetchState::Ready(Some(pr)) = &self.state else {
            return;
        };

        let label = format!("{} #{}", S::pr_icon(), pr.number);
        let (fg, bg) = pr.state.style::<S>();

        // The CI status, when enabled and meaningful, renders as a coloured
        // dot tucked into the same segment right after the PR number. It's
        // only shown while a PR is still in progress - the checks are stale
        // or irrelevant once a PR is merged or closed.
        let marker = (self.show_status && pr.state.is_open())
            .then(|| {
                pr.checks
                    .map(|status| (S::pr_status_icon(), status.fg::<S>()))
            })
            .flatten();

        powerline.add_hyperlink_segment(&label, &pr.url, Style::simple(fg, bg), marker);
    }
}

/// Resolves the current branch name and repository root in a single, fast,
/// network-free git invocation run in `cwd`. Returns `None` outside a git
/// repository.
pub(crate) fn current_branch_and_root_from(cwd: &Path) -> Option<(String, PathBuf)> {
    let output = Command::new("git")
        .current_dir(cwd)
        .args(["rev-parse", "--abbrev-ref", "HEAD", "--show-toplevel"])
        .output()
        .ok()?;

    if !output.status.success() {
        return None;
    }

    let text = String::from_utf8(output.stdout).ok()?;
    let mut lines = text.lines();
    let branch = lines.next()?.trim().to_string();
    let root = lines.next()?.trim();

    if branch.is_empty() || root.is_empty() {
        return None;
    }

    Some((branch, PathBuf::from(root)))
}

/// Performs the blocking `gh` lookup for a branch. Returns `None` both when the
/// lookup fails and when there is no PR for the branch; the server records that
/// as `Ready(None)` so it isn't retried on every prompt.
pub(crate) fn fetch_pr(branch: &str, repo_dir: &Path) -> Option<PrInfo> {
    let output = Command::new("gh")
        .current_dir(repo_dir)
        .args([
            "pr",
            "view",
            branch,
            "--json",
            "number,url,state,isDraft,statusCheckRollup",
        ])
        .output()
        .ok()?;

    // A non-zero exit usually just means there's no PR for this branch.
    if !output.status.success() {
        return None;
    }

    let gh: GhPr = serde_json::from_slice(&output.stdout).ok()?;

    let state = pr_state(&gh.state, gh.is_draft);

    Some(PrInfo {
        number: gh.number,
        url: gh.url,
        state,
        checks: aggregate(&gh.status_check_rollup),
    })
}

/// Collapses individual checks into a single status. Failure beats pending,
/// which beats success. Returns `None` when there are no meaningful checks, so
/// the dot renders nothing rather than misleading the reader.
fn aggregate(checks: &[CheckItem]) -> Option<CheckStatus> {
    let mut any_pending = false;
    let mut any_success = false;

    for check in checks {
        match check.outcome() {
            CheckOutcome::Failure => return Some(CheckStatus::Failure),
            CheckOutcome::Pending => any_pending = true,
            CheckOutcome::Success => any_success = true,
            CheckOutcome::Neutral => {}
        }
    }

    if any_pending {
        Some(CheckStatus::Pending)
    } else if any_success {
        Some(CheckStatus::Success)
    } else {
        None
    }
}

enum CheckOutcome {
    Success,
    Failure,
    Pending,
    Neutral,
}

/// A single entry in GitHub's `statusCheckRollup`. Check runs report
/// `status`/`conclusion`; legacy status contexts report `state`.
#[derive(Deserialize)]
struct CheckItem {
    #[serde(default)]
    status: Option<String>,
    #[serde(default)]
    conclusion: Option<String>,
    #[serde(default)]
    state: Option<String>,
}

impl CheckItem {
    fn outcome(&self) -> CheckOutcome {
        // Legacy commit-status contexts carry a `state` instead of a status/
        // conclusion pair.
        if let Some(state) = &self.state {
            return match state.as_str() {
                "SUCCESS" => CheckOutcome::Success,
                "PENDING" | "EXPECTED" => CheckOutcome::Pending,
                _ => CheckOutcome::Failure, // FAILURE, ERROR
            };
        }

        match self.status.as_deref() {
            Some("COMPLETED") => match self.conclusion.as_deref() {
                Some("SUCCESS") => CheckOutcome::Success,
                // Skipped / neutral checks shouldn't tip the dot either way.
                Some("SKIPPED") | Some("NEUTRAL") => CheckOutcome::Neutral,
                // FAILURE, TIMED_OUT, CANCELLED, ACTION_REQUIRED, STARTUP_FAILURE
                _ => CheckOutcome::Failure,
            },
            // QUEUED, IN_PROGRESS, WAITING, PENDING, REQUESTED, ...
            Some(_) => CheckOutcome::Pending,
            None => CheckOutcome::Neutral,
        }
    }
}

/// Derives the display state from the `state` and `isDraft` fields of a `gh`
/// response. A draft PR is reported as OPEN with `isDraft: true`, but the flag
/// stays set once the draft is merged or closed, so the terminal state wins.
fn pr_state(state: &str, is_draft: bool) -> PrState {
    match state {
        "MERGED" => PrState::Merged,
        "CLOSED" => PrState::Closed,
        _ if is_draft => PrState::Draft,
        _ => PrState::Open,
    }
}

/// Shape of the `gh pr view --json ...` response we care about.
#[derive(Deserialize)]
struct GhPr {
    number: u64,
    url: String,
    state: String,
    #[serde(rename = "isDraft")]
    is_draft: bool,
    #[serde(rename = "statusCheckRollup", default)]
    status_check_rollup: Vec<CheckItem>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn draft_flag_only_applies_while_the_pr_is_open() {
        assert!(matches!(pr_state("OPEN", true), PrState::Draft));
        assert!(matches!(pr_state("OPEN", false), PrState::Open));
        assert!(matches!(pr_state("CLOSED", true), PrState::Closed));
        assert!(matches!(pr_state("MERGED", true), PrState::Merged));
    }

    #[test]
    fn closed_and_merged_prs_hide_the_check_status() {
        assert!(pr_state("OPEN", true).is_open());
        assert!(pr_state("OPEN", false).is_open());
        assert!(!pr_state("CLOSED", true).is_open());
        assert!(!pr_state("MERGED", true).is_open());
    }
}
