use std::collections::hash_map::DefaultHasher;
use std::env;
use std::fs::{self, File};
use std::hash::{Hash, Hasher};
use std::io::Write as _;
use std::marker::PhantomData;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

use crate::colors::Color;
use crate::themes::DefaultColors;
use crate::{Powerline, Style};

use super::Module;

/// How long a cached lookup stays fresh. Prompts rendered within this window
/// reuse the cache and never touch the network.
const CACHE_TTL: Duration = Duration::from_secs(60);
/// Debounce window for the background refresher, so several prompts rendered in
/// quick succession don't each spawn their own `gh` process.
const REFRESH_DEBOUNCE: Duration = Duration::from_secs(20);

/// Branches that never have a PR of their own - skip all work for these.
const SKIP_BRANCHES: &[&str] = &["develop", "main", "master", "HEAD"];

pub struct Pr<S> {
    /// Whether to append the CI check-status dot after the PR number.
    show_status: bool,
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
            scheme: PhantomData,
        }
    }
}

#[derive(Serialize, Deserialize, Clone, Copy)]
#[serde(rename_all = "snake_case")]
enum PrState {
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
enum CheckStatus {
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
struct PrInfo {
    number: u64,
    url: String,
    state: PrState,
    /// Aggregate CI status. `None` means there are no meaningful checks, so the
    /// dot is hidden rather than shown misleadingly. Defaulted for forward
    /// compatibility with caches written before this field existed.
    #[serde(default)]
    checks: Option<CheckStatus>,
}

#[derive(Serialize, Deserialize)]
struct PrCache {
    branch: String,
    /// `None` means "looked up, but no PR exists for this branch" - cached so we
    /// don't re-query on every prompt.
    pr: Option<PrInfo>,
    fetched_at: u64,
}

impl<S: PrScheme> Module for Pr<S> {
    fn append_segments(&mut self, powerline: &mut Powerline) {
        let Some((branch, repo_root)) = current_branch_and_root() else {
            return;
        };
        if SKIP_BRANCHES.contains(&branch.as_str()) {
            return;
        }

        // Without a cache directory we'd have to fetch synchronously, which
        // could block the prompt on a network request - so bail instead.
        let Some(cache_path) = cache_path_for(&repo_root, &branch) else {
            return;
        };

        let cache = read_cache(&cache_path).filter(|c| c.branch == branch);

        // Refresh in the background when the cache is missing or stale. This
        // never blocks rendering - the result is picked up by a later prompt.
        if cache.as_ref().is_none_or(|c| is_stale(c.fetched_at)) {
            spawn_refresh(&branch, &repo_root, &cache_path);
        }

        // Render whatever we have right now (possibly slightly stale).
        if let Some(PrCache { pr: Some(pr), .. }) = cache {
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
}

/// Resolves the current branch name and repository root in a single, fast,
/// network-free git invocation. Returns `None` outside a git repository.
fn current_branch_and_root() -> Option<(String, PathBuf)> {
    let output = Command::new("git")
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

fn cache_path_for(repo_root: &Path, branch: &str) -> Option<PathBuf> {
    let base = crate::platform::cache_dir()?;

    let mut hasher = DefaultHasher::new();
    repo_root.hash(&mut hasher);
    branch.hash(&mut hasher);

    Some(
        base.join("superline")
            .join(format!("pr-{:016x}.json", hasher.finish())),
    )
}

fn read_cache(path: &Path) -> Option<PrCache> {
    serde_json::from_reader(File::open(path).ok()?).ok()
}

fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

fn is_stale(fetched_at: u64) -> bool {
    now_secs().saturating_sub(fetched_at) >= CACHE_TTL.as_secs()
}

/// True if a refresh was kicked off recently enough that we should let it
/// finish rather than spawning another one.
fn refresh_in_flight(lock_path: &Path) -> bool {
    fs::metadata(lock_path)
        .and_then(|meta| meta.modified())
        .ok()
        .and_then(|modified| modified.elapsed().ok())
        .is_some_and(|elapsed| elapsed < REFRESH_DEBOUNCE)
}

/// Spawns a detached process to refresh the cache. The child's stdio is
/// redirected to null so the shell's command substitution doesn't block
/// waiting on the inherited pipe.
fn spawn_refresh(branch: &str, repo_root: &Path, cache_path: &Path) {
    let lock_path = cache_path.with_extension("lock");
    if refresh_in_flight(&lock_path) {
        return;
    }

    if let Some(parent) = cache_path.parent() {
        let _ = fs::create_dir_all(parent);
    }
    // Touch the lock up front to debounce concurrent prompts.
    let _ = File::create(&lock_path);

    let Ok(exe) = env::current_exe() else {
        return;
    };

    let _ = Command::new(exe)
        .arg("refresh-pr")
        .args(["--branch", branch])
        .arg("--repo-dir")
        .arg(repo_root)
        .arg("--cache")
        .arg(cache_path)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn();
}

/// Performs the blocking `gh` lookup and writes the cache. Invoked by the
/// hidden `refresh-pr` subcommand from the detached process spawned above.
/// Always fetches the check status too - rendering it is a display-time choice,
/// so the cache stays the same regardless of config.
pub fn refresh_pr(branch: &str, repo_dir: &Path, cache_path: &Path) {
    let cache = PrCache {
        branch: branch.to_string(),
        pr: fetch_pr(branch, repo_dir),
        fetched_at: now_secs(),
    };

    write_cache(cache_path, &cache);
    let _ = fs::remove_file(cache_path.with_extension("lock"));
}

fn fetch_pr(branch: &str, repo_dir: &Path) -> Option<PrInfo> {
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

fn write_cache(path: &Path, cache: &PrCache) {
    if let Some(parent) = path.parent() {
        let _ = fs::create_dir_all(parent);
    }

    // Write to a temp file and rename so a concurrent reader never sees a
    // half-written cache.
    let tmp = path.with_extension("tmp");
    if let Ok(mut file) = File::create(&tmp) {
        if serde_json::to_writer(&mut file, cache).is_ok() && file.flush().is_ok() {
            let _ = fs::rename(&tmp, path);
        }
    }
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
