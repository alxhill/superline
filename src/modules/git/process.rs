use std::path::Path;
use std::process::Command;

use super::{detached_label, preferred_branch, GitStats};

pub fn get_first_number(s: &str) -> u32 {
    s.chars()
        .take_while(|x| x.is_ascii_digit())
        .flat_map(|x| x.to_digit(10))
        .fold(0, |acc, x| 10 * acc + x)
}

pub fn extract_ahead_behind(s: &str) -> (u32, u32) {
    let extract_number = |pos: usize, offset: usize| -> u32 {
        let s = s.get((pos + offset)..).unwrap();
        get_first_number(s)
    };
    let ahead = s
        .find("ahead")
        .map(|pos| extract_number(pos, 6))
        .unwrap_or(0);
    let behind = s
        .find("behind")
        .map(|pos| extract_number(pos, 7))
        .unwrap_or(0);
    (ahead, behind)
}

pub fn get_branch_name(s: &str) -> Option<&str> {
    if let Some(rest) = s.get(3..) {
        let mut end: usize = 0;
        if let Some(pos) = rest.find("...") {
            end = pos
        } else {
            let mut text = rest.chars();
            while let Some(c) = text.next() {
                end += 1;
                if c.is_whitespace() {
                    if Some('[') != text.next() {
                        return None;
                    }
                    break;
                }
            }
        }
        rest.get(..end)
    } else {
        None
    }
}

/// `None` when `git` could not be run at all; `Some("Big Bang")` when it ran
/// but found no commit (an unborn HEAD).
pub fn get_detached_branch_name() -> Option<String> {
    let child = Command::new("git")
        .args(["rev-parse", "--short", "HEAD"])
        .output()
        .ok()?;

    if child.status.success() {
        let hash = std::str::from_utf8(&child.stdout)
            .ok()?
            .split('\n')
            .next()?;
        Some(detached_label(branch_at_head(), hash))
    } else {
        Some(String::from("Big Bang"))
    }
}

/// The branch whose tip is HEAD, for labelling a detached HEAD. Local branches
/// are preferred; remote-tracking branches (`origin/main`) are only consulted
/// when no local branch matches.
fn branch_at_head() -> Option<String> {
    ["refs/heads/", "refs/remotes/"]
        .into_iter()
        .find_map(|prefix| {
            let output = Command::new("git")
                .args([
                    "for-each-ref",
                    "--points-at=HEAD",
                    "--format=%(refname:short)",
                    prefix,
                ])
                .output()
                .ok()
                .filter(|out| out.status.success())?;
            let stdout = String::from_utf8(output.stdout).ok()?;
            preferred_branch(
                stdout
                    .lines()
                    .map(str::trim)
                    // `origin/HEAD` is a symbolic ref, not a branch of its own.
                    .filter(|name| !name.is_empty() && !name.ends_with("/HEAD"))
                    .map(ToOwned::to_owned),
            )
        })
}

/// Whether the repository has any remote configured. This is deliberately
/// repo-level rather than reading the `## local...remote` separator out of
/// `git status -b --porcelain`: that separator survives as `...origin/x [gone]`
/// once the upstream branch is deleted, and it is absent on a branch that was
/// never pushed even though the repo has a remote.
fn has_remote() -> bool {
    Command::new("git")
        .arg("remote")
        .output()
        .is_ok_and(|out| out.status.success() && !out.stdout.iter().all(u8::is_ascii_whitespace))
}

/// Falls back to the gitoxide backend when `git` cannot be run at all, so a
/// refresh never panics just because `git` is missing from `PATH`.
pub fn run_git(path: &Path) -> GitStats {
    try_run_git().unwrap_or_else(|| super::gitoxide::run_git(path))
}

fn try_run_git() -> Option<GitStats> {
    let output = Command::new("git")
        .args(["status", "--porcelain", "-b"])
        .output()
        .ok()?
        .stdout;

    let mut lines = output.split(|x| *x == (b'\n'));
    let branch_line = std::str::from_utf8(lines.next()?).ok()?;

    let remote = has_remote();
    let remote_url = remote_web_url_of();

    let mut ahead = 0;
    let mut behind = 0;
    let mut non_staged = 0;
    let mut staged = 0;
    let mut conflicted = 0;
    let mut untracked = 0;

    let branch_name = {
        if let Some(branch_name) = get_branch_name(branch_line) {
            if let Some(info) = branch_line
                .find('[')
                .map(|pos| branch_line.get(pos..).unwrap())
            {
                let (a, b) = extract_ahead_behind(info);
                ahead = a;
                behind = b;
            }
            String::from(branch_name)
        } else {
            get_detached_branch_name()?
        }
    };
    let mut add_file = |entry: &str| {
        match entry {
            "??" => untracked += 1,
            "DD" | "AU" | "UD" | "UA" | "UU" | "DU" | "AA" => conflicted += 1,
            _ => {
                let mut chars = entry.chars();
                let a = chars.next().expect("invalid file status");
                let b = chars.next().expect("invalid file status");
                if b != ' ' {
                    non_staged += 1;
                }
                if a != ' ' {
                    staged += 1;
                }
            }
        };
    };
    for op in lines.flat_map(|line| line.get(..2)) {
        add_file(std::str::from_utf8(op).unwrap());
    }

    Some(super::GitStats {
        untracked,
        ahead,
        behind,
        non_staged,
        staged,
        conflicted,
        remote,
        remote_url,
        branch_name,
    })
}

/// The browser URL of the preferred remote's fetch URL, if it has one.
fn remote_web_url_of() -> Option<String> {
    let names = Command::new("git").arg("remote").output().ok()?;
    if !names.status.success() {
        return None;
    }
    let names = String::from_utf8(names.stdout).ok()?;
    let name = super::preferred_remote(names.lines().map(str::trim).filter(|n| !n.is_empty()))?;
    let url = Command::new("git")
        .args(["remote", "get-url", name])
        .output()
        .ok()?;
    if !url.status.success() {
        return None;
    }
    super::remote_web_url(std::str::from_utf8(&url.stdout).ok()?)
}
