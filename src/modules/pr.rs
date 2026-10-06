use std::marker::PhantomData;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::cache::{hash_id, Cached, Lookup, Source};
use crate::claude_code::PullRequest;
use crate::colors::{self, Color};
use crate::config::SegmentPadding;
use crate::powerline::Marker;
use crate::themes::DefaultColors;
use crate::utils::join_non_empty;
use crate::{Powerline, Style};

use super::{DefaultPadding, Module};

/// Branches that never have a PR of their own - skip all work for these.
const SKIP_BRANCHES: &[&str] = &["develop", "main", "master", "HEAD"];

pub struct Pr<S> {
    /// Whether to append the CI check-status dot after the PR number.
    show_status: bool,
    /// Whether hovering over the dot in iTerm2 lists the checks by outcome.
    hover: bool,
    /// The PR Claude Code passed to its status line, shown when the `gh`
    /// lookup has none.
    reported: Option<PrInfo>,
    scheme: PhantomData<S>,
}

pub trait PrScheme: DefaultColors {
    const PR_ICON: &'static str = "\u{ea64}"; // nf-cod-git_pull_request
    const PR_STATUS_ICON: &'static str = "\u{25cf}"; // ● black circle
    const PR_DIFF_ICON: &'static str = "\u{eafd}"; // nf-cod-git_compare
    const PR_DIFF_ADDED_FG: Color = colors::green();
    const PR_DIFF_REMOVED_FG: Color = colors::red();

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
        Self::PR_ICON
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
        Self::PR_STATUS_ICON
    }

    fn pr_diff_added_fg() -> Color {
        Self::PR_DIFF_ADDED_FG
    }
    fn pr_diff_removed_fg() -> Color {
        Self::PR_DIFF_REMOVED_FG
    }
    fn pr_diff_bg() -> Color {
        Self::default_bg()
    }
    fn pr_diff_icon() -> &'static str {
        Self::PR_DIFF_ICON
    }
}

impl<S: PrScheme> Default for Pr<S> {
    fn default() -> Self {
        Self::new(true, true)
    }
}

impl<S: PrScheme> Pr<S> {
    pub fn new(show_status: bool, hover: bool) -> Pr<S> {
        Pr {
            show_status,
            hover,
            reported: None,
            scheme: PhantomData,
        }
    }

    pub fn with_claude_code_pr(mut self, pr: Option<&PullRequest>) -> Pr<S> {
        self.reported = pr.and_then(PrInfo::from_claude_code);
        self
    }
}

#[derive(Serialize, Deserialize, Clone, Copy)]
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
#[derive(Serialize, Deserialize, Clone, Copy)]
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

#[derive(Serialize, Deserialize)]
pub struct PrInfo {
    number: u64,
    url: String,
    state: PrState,
    /// Aggregate CI status. `None` means there are no meaningful checks, so the
    /// dot is hidden rather than shown misleadingly. Defaulted for forward
    /// compatibility with caches written before this field existed.
    #[serde(default)]
    checks: Option<CheckStatus>,
    /// Lines added and deleted across the PR. `None` for caches written
    /// before this field existed and for the PR Claude Code reports.
    #[serde(default)]
    diff: Option<Diff>,
    /// Every check behind `checks`, by name, for the dot's hover text. Empty
    /// for caches written before this field existed and for the PR Claude
    /// Code reports.
    #[serde(default)]
    check_runs: Vec<Check>,
}

/// One check run or commit status context and how it ended up.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Check {
    name: String,
    outcome: CheckOutcome,
}

#[derive(Serialize, Deserialize, Clone, Copy)]
pub struct Diff {
    additions: u64,
    deletions: u64,
}

impl PrInfo {
    /// Claude Code reports the review state rather than the CI checks, so no
    /// status dot is shown for it.
    fn from_claude_code(pr: &PullRequest) -> Option<PrInfo> {
        Some(PrInfo {
            number: pr.number?,
            url: pr.url.clone()?,
            state: if pr.is_draft() {
                PrState::Draft
            } else {
                PrState::Open
            },
            checks: None,
            diff: None,
            check_runs: Vec::new(),
        })
    }
}

/// The PR for one branch of one repository, looked up through `gh`.
#[derive(Clone, Serialize, Deserialize)]
pub struct PrLookup {
    pub branch: String,
    pub repo_root: PathBuf,
}

impl Source for PrLookup {
    /// `None` means "looked up, but no PR exists for this branch" - cached so we
    /// don't re-query on every prompt.
    type Value = Option<PrInfo>;
    const KIND: &'static str = "pr";
    /// Prompts rendered within this window reuse the cache and never touch the
    /// network.
    const TTL: Duration = Duration::from_secs(60);

    fn cache_id(&self) -> String {
        hash_id(&(&self.repo_root, &self.branch))
    }

    /// Always fetches the check status and diff too - rendering it is a display-time
    /// choice, so the cache stays the same regardless of config.
    fn fetch(&self) -> Option<Option<PrInfo>> {
        Some(fetch_pr(&self.branch, &self.repo_root))
    }
}

impl<S: PrScheme> Module for Pr<S> {
    fn default_padding(&self) -> DefaultPadding {
        SegmentPadding::Large.into()
    }

    fn append_segments(&mut self, powerline: &mut Powerline) {
        let Some(pr) = cached_pr().or_else(|| self.reported.take()) else {
            return;
        };

        let label = join_non_empty([S::pr_icon(), format!("#{}", pr.number).as_str()]);
        let (fg, bg) = pr.state.style::<S>();

        // The CI status, when enabled and meaningful, renders as a coloured
        // dot tucked into the same segment right after the PR number. It's
        // only shown while a PR is still in progress - the checks are stale
        // or irrelevant once a PR is merged or closed.
        let marker = (self.show_status && pr.state.is_open())
            .then(|| {
                pr.checks
                    .map(|status| Marker::new(S::pr_status_icon(), status.fg::<S>()))
            })
            .flatten()
            .filter(|marker| !marker.glyph.is_empty());
        let note = marker
            .filter(|_| self.hover)
            .and_then(|_| hover_note(&pr.check_runs));
        let marker = marker.map(|marker| marker.with_note(note.as_deref()));

        powerline.add_hyperlink_segment(&label, &pr.url, Style::simple(fg, bg), marker);
    }
}

/// The added and deleted line counts of the current branch's PR, from the
/// same cached lookup as [`Pr`].
pub struct PrDiff<S> {
    scheme: PhantomData<S>,
}

impl<S: PrScheme> Default for PrDiff<S> {
    fn default() -> Self {
        Self::new()
    }
}

impl<S: PrScheme> PrDiff<S> {
    pub fn new() -> PrDiff<S> {
        PrDiff {
            scheme: PhantomData,
        }
    }
}

impl<S: PrScheme> Module for PrDiff<S> {
    fn default_padding(&self) -> DefaultPadding {
        SegmentPadding::Large.into()
    }

    fn append_segments(&mut self, powerline: &mut Powerline) {
        let Some(diff) = cached_pr().and_then(|pr| pr.diff) else {
            return;
        };
        powerline.add_two_tone_segment(
            &join_non_empty([S::pr_diff_icon(), &format!("+{}", diff.additions)]),
            &format!("-{}", diff.deletions),
            S::pr_diff_removed_fg(),
            Style::simple(S::pr_diff_added_fg(), S::pr_diff_bg()),
        );
    }
}

/// The current branch's PR as last looked up through `gh`, possibly slightly
/// stale; a missing or stale lookup is refreshed for a later prompt. There is
/// no loading state: segments simply appear once the result is in.
fn cached_pr() -> Option<PrInfo> {
    let (branch, repo_root) = current_branch_and_root()
        .filter(|(branch, _)| !SKIP_BRANCHES.contains(&branch.as_str()))?;
    match Cached::new(PrLookup { branch, repo_root }).load() {
        Lookup::Ready(pr) => pr,
        Lookup::Loading | Lookup::Unavailable => None,
    }
}

/// Resolves the current branch name and repository root by walking up to the
/// `.git` entry and reading `HEAD`. Returns `None` outside a git repository.
fn current_branch_and_root() -> Option<(String, PathBuf)> {
    let (root, _) = super::git::find_git_dir()?;
    let branch = super::git::head_branch(&root)?;
    (!branch.is_empty()).then_some((branch, root))
}

fn fetch_pr(branch: &str, repo_dir: &Path) -> Option<PrInfo> {
    let output = Command::new("gh")
        .current_dir(repo_dir)
        .args([
            "pr",
            "view",
            branch,
            "--json",
            "number,url,state,isDraft,statusCheckRollup,additions,deletions",
        ])
        .output()
        .ok()?;

    // A non-zero exit usually just means there's no PR for this branch.
    if !output.status.success() {
        return None;
    }

    let gh: GhPr = serde_json::from_slice(&output.stdout).ok()?;

    let state = pr_state(&gh.state, gh.is_draft);
    let check_runs: Vec<Check> = gh
        .status_check_rollup
        .iter()
        .map(CheckItem::check)
        .collect();

    Some(PrInfo {
        number: gh.number,
        url: gh.url,
        state,
        checks: aggregate(check_runs.iter().map(|check| check.outcome)),
        diff: Some(Diff {
            additions: gh.additions,
            deletions: gh.deletions,
        }),
        check_runs,
    })
}

/// What hovering over the CI dot shows: how many checks failed, are still
/// running, passed and were skipped, each followed by their names, worst
/// first. `None` when there are no checks to list, e.g. for a cache written
/// before check names were recorded.
fn hover_note(checks: &[Check]) -> Option<String> {
    let groups: Vec<String> = [
        (CheckOutcome::Failure, "failed"),
        (CheckOutcome::Pending, "pending"),
        (CheckOutcome::Success, "passed"),
        (CheckOutcome::Neutral, "skipped"),
    ]
    .into_iter()
    .filter_map(|(outcome, verb)| {
        let matching: Vec<&Check> = checks
            .iter()
            .filter(|check| check.outcome == outcome)
            .collect();
        if matching.is_empty() {
            return None;
        }
        let names: Vec<&str> = matching
            .iter()
            .map(|check| check.name.trim())
            .filter(|name| !name.is_empty())
            .collect();
        let count = matching.len();
        Some(if names.is_empty() {
            format!("{count} {verb}")
        } else {
            format!("{count} {verb}: {}", names.join(", "))
        })
    })
    .collect();
    (!groups.is_empty()).then(|| groups.join(" · "))
}

/// Collapses individual checks into a single status. Failure beats pending,
/// which beats success. Returns `None` when there are no meaningful checks, so
/// the dot renders nothing rather than misleading the reader.
fn aggregate(outcomes: impl IntoIterator<Item = CheckOutcome>) -> Option<CheckStatus> {
    let mut any_pending = false;
    let mut any_success = false;

    for outcome in outcomes {
        match outcome {
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

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq)]
#[serde(rename_all = "snake_case")]
enum CheckOutcome {
    Success,
    Failure,
    Pending,
    Neutral,
}

/// A single entry in GitHub's `statusCheckRollup`. Check runs report
/// `name` and `status`/`conclusion`; legacy status contexts report `context`
/// and `state`.
#[derive(Deserialize)]
struct CheckItem {
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    context: Option<String>,
    #[serde(default)]
    status: Option<String>,
    #[serde(default)]
    conclusion: Option<String>,
    #[serde(default)]
    state: Option<String>,
}

impl CheckItem {
    fn check(&self) -> Check {
        Check {
            name: self
                .name
                .as_deref()
                .or(self.context.as_deref())
                .unwrap_or_default()
                .to_string(),
            outcome: self.outcome(),
        }
    }

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
    #[serde(default)]
    additions: u64,
    #[serde(default)]
    deletions: u64,
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

    fn rollup(json: &str) -> Vec<Check> {
        serde_json::from_str::<Vec<CheckItem>>(json)
            .unwrap()
            .iter()
            .map(CheckItem::check)
            .collect()
    }

    #[test]
    fn checks_are_named_by_check_run_or_status_context() {
        let checks = rollup(
            r#"[
                {"__typename":"CheckRun","name":"test (macos)","status":"COMPLETED","conclusion":"FAILURE","workflowName":"CI"},
                {"__typename":"StatusContext","context":"ci/circleci","state":"PENDING"},
                {"__typename":"CheckRun","name":"fmt","status":"COMPLETED","conclusion":"SKIPPED"},
                {"__typename":"CheckRun","status":"IN_PROGRESS"}
            ]"#,
        );
        let names: Vec<(&str, CheckOutcome)> = checks
            .iter()
            .map(|check| (check.name.as_str(), check.outcome))
            .collect();
        assert_eq!(
            names,
            [
                ("test (macos)", CheckOutcome::Failure),
                ("ci/circleci", CheckOutcome::Pending),
                ("fmt", CheckOutcome::Neutral),
                ("", CheckOutcome::Pending),
            ]
        );
    }

    #[test]
    fn hover_note_lists_checks_worst_first() {
        let checks = rollup(
            r#"[
                {"name":"build","status":"COMPLETED","conclusion":"SUCCESS"},
                {"name":"lint","status":"COMPLETED","conclusion":"FAILURE"},
                {"name":"deploy","status":"COMPLETED","conclusion":"SKIPPED"},
                {"name":"e2e","status":"IN_PROGRESS"},
                {"context":"docs","state":"SUCCESS"},
                {"name":"test","status":"COMPLETED","conclusion":"TIMED_OUT"}
            ]"#,
        );
        assert_eq!(
            hover_note(&checks).as_deref(),
            Some(
                "2 failed: lint, test · 1 pending: e2e · 2 passed: build, docs · 1 skipped: deploy"
            )
        );
        assert!(matches!(
            aggregate(checks.iter().map(|check| check.outcome)),
            Some(CheckStatus::Failure)
        ));
    }

    #[test]
    fn hover_note_counts_unnamed_checks_and_skips_empty_groups() {
        let checks = rollup(
            r#"[
                {"name":"build","status":"COMPLETED","conclusion":"SUCCESS"},
                {"status":"COMPLETED","conclusion":"SUCCESS"},
                {"status":"QUEUED"}
            ]"#,
        );
        assert_eq!(
            hover_note(&checks).as_deref(),
            Some("1 pending · 2 passed: build")
        );
        assert_eq!(hover_note(&[]), None);
    }

    #[test]
    fn caches_without_check_names_still_load() {
        let pr: PrInfo = serde_json::from_str(
            r#"{"number":1,"url":"https://example.com/1","state":"open","checks":"success"}"#,
        )
        .unwrap();
        assert!(pr.check_runs.is_empty());
        assert!(matches!(pr.checks, Some(CheckStatus::Success)));

        let round_trip: PrInfo = serde_json::from_str(
            &serde_json::to_string(&PrInfo {
                check_runs: rollup(
                    r#"[{"name":"lint","status":"COMPLETED","conclusion":"FAILURE"}]"#,
                ),
                ..pr
            })
            .unwrap(),
        )
        .unwrap();
        assert_eq!(
            hover_note(&round_trip.check_runs).as_deref(),
            Some("1 failed: lint")
        );
    }

    #[test]
    fn closed_and_merged_prs_hide_the_check_status() {
        assert!(pr_state("OPEN", true).is_open());
        assert!(pr_state("OPEN", false).is_open());
        assert!(!pr_state("CLOSED", true).is_open());
        assert!(!pr_state("MERGED", true).is_open());
    }
}
