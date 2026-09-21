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

/// Whether the repository has any remote configured, and the browser URL of
/// the preferred one.
///
/// Read in-process through gitoxide, which answers both from the repository's
/// config in well under a millisecond. Asking `git` instead costs a second
/// child process - around 30ms on Windows, comparable to the `git status` call
/// this backend exists for - so it is kept only as the fallback for a
/// repository gitoxide cannot open, which is the very case the CLI backend is
/// here to cover.
///
/// Either way the answer is deliberately repo-level rather than read from the
/// `## local...remote` separator in `git status -b --porcelain`: that
/// separator survives as `...origin/x [gone]` once the upstream branch is
/// deleted, and is absent on a branch that was never pushed even though the
/// repo has a remote.
pub(super) fn remote_info(path: &Path) -> (bool, Option<String>) {
    super::gitoxide::remote_info(path).unwrap_or_else(remote_info_from_cli)
}

/// [`remote_info`] via a single `git remote -v`. Both remote names and their
/// URLs come out of the one call: `git remote` for the names plus
/// `git remote get-url` for the chosen one would cost two more spawns.
fn remote_info_from_cli() -> (bool, Option<String>) {
    let Ok(output) = Command::new("git").args(["remote", "-v"]).output() else {
        return (false, None);
    };
    if !output.status.success() {
        return (false, None);
    }
    let Ok(listing) = String::from_utf8(output.stdout) else {
        return (false, None);
    };

    let (remote, url) = parse_remote_listing(&listing);
    (remote, url.and_then(super::remote_web_url))
}

/// Pulls the remote names and the preferred remote's fetch URL out of
/// `git remote -v` output.
///
/// Each remote contributes a `name<TAB>url (fetch)` and a
/// `name<TAB>url (push)` line, except one with no URL configured, which is
/// listed as a bare `name<TAB>` - so the names seen here are exactly the ones
/// plain `git remote` prints, and such a remote correctly yields no URL.
fn parse_remote_listing(listing: &str) -> (bool, Option<&str>) {
    let entries: Vec<(&str, &str)> = listing
        .lines()
        .filter_map(|line| line.split_once('\t'))
        .map(|(name, rest)| (name.trim(), rest.trim_end()))
        .filter(|(name, _)| !name.is_empty())
        .collect();

    let url = super::preferred_remote(entries.iter().map(|(name, _)| *name)).and_then(|wanted| {
        entries
            .iter()
            .find_map(|(name, rest)| (*name == wanted).then(|| rest.strip_suffix(" (fetch)"))?)
    });

    (!entries.is_empty(), url)
}

/// Falls back to the gitoxide backend when `git` cannot be run at all, so a
/// refresh never panics just because `git` is missing from `PATH`.
pub fn run_git(path: &Path) -> GitStats {
    try_run_git(path).unwrap_or_else(|| super::gitoxide::run_git(path))
}

fn try_run_git(path: &Path) -> Option<GitStats> {
    let output = Command::new("git")
        .args(["status", "--porcelain", "-b"])
        .output()
        .ok()?
        .stdout;

    let mut lines = output.split(|x| *x == (b'\n'));
    let branch_line = std::str::from_utf8(lines.next()?).ok()?;

    let (remote, remote_url) = remote_info(path);

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

#[cfg(test)]
mod tests {
    use super::parse_remote_listing;

    #[test]
    fn a_repo_without_remotes_has_neither_flag_nor_url() {
        assert_eq!(parse_remote_listing(""), (false, None));
    }

    #[test]
    fn the_fetch_url_of_the_preferred_remote_is_picked() {
        let listing = "\
upstream\thttps://github.com/them/repo.git (fetch)
upstream\thttps://github.com/them/repo.git (push)
origin\tgit@github.com:me/repo.git (fetch)
origin\tgit@github.com:me/repo.git (push)
";
        assert_eq!(
            parse_remote_listing(listing),
            (true, Some("git@github.com:me/repo.git"))
        );
    }

    #[test]
    fn a_remote_without_a_url_still_counts_as_a_remote() {
        assert_eq!(parse_remote_listing("origin\t\n"), (true, None));
    }

    #[test]
    fn a_push_only_remote_yields_no_fetch_url() {
        assert_eq!(
            parse_remote_listing("origin\thttps://example.com/r.git (push)\n"),
            (true, None)
        );
    }
}
