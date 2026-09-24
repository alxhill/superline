use std::path::Path;

use gix::status::index_worktree::Item as IndexWorktreeItem;
use gix::status::plumbing::index_as_worktree::EntryStatus;
use gix::status::Item;

use super::{detached_label, preferred_branch, preferred_remote, remote_web_url, GitStats};

/// gitoxide (pure-Rust) git backend. Produces the same [`GitStats`] the libgit
/// and CLI backends do: a count of staged / non-staged / untracked / conflicted
/// paths, whether the repository has a remote configured, and the ahead/behind
/// distance from the upstream tracking branch.
pub fn run_git(path: &Path) -> GitStats {
    let repo = discover(path).unwrap();

    let (mut untracked, mut staged, mut non_staged, mut conflicted) = (0u32, 0, 0, 0);

    // Walk the repository status: HEAD-tree-vs-index entries are staged, while
    // index-vs-worktree entries are either modifications, conflicts, renames,
    // or untracked directory contents. Untracked listing follows the repo's
    // `status.showUntrackedFiles` config (collapsed directories by default),
    // matching the other backends.
    let status = repo
        .status(gix::progress::Discard)
        .unwrap()
        .into_iter(None)
        .unwrap();

    for item in status {
        let Ok(item) = item else { continue };
        match item {
            Item::TreeIndex(_) => staged += 1,
            Item::IndexWorktree(IndexWorktreeItem::Modification { status, .. }) => {
                if matches!(status, EntryStatus::Conflict { .. }) {
                    conflicted += 1;
                } else {
                    non_staged += 1;
                }
            }
            Item::IndexWorktree(IndexWorktreeItem::Rewrite { .. }) => non_staged += 1,
            Item::IndexWorktree(IndexWorktreeItem::DirectoryContents { entry, .. }) => {
                if entry.status == gix::dir::entry::Status::Untracked
                    && !is_hollow_untracked_dir(&repo, &entry)
                {
                    untracked += 1;
                }
            }
        }
    }

    let head_name = repo.head_name().ok().flatten();
    // `None` for an unborn branch (no commits yet) as well as a missing HEAD.
    let head_id = repo.head_id().ok();

    let branch_name = match (&head_name, &head_id) {
        // On a branch with at least one commit: show the short branch name.
        (Some(name), Some(_)) => name.shorten().to_string(),
        // Detached HEAD: the branch whose tip this is (if any) and the hash.
        (None, Some(id)) => detached_label(branch_at(&repo, id), &id.shorten_or_id().to_string()),
        // Unborn branch / no HEAD: match the libgit & CLI "Big Bang" label.
        _ => String::from("Big Bang"),
    };

    // Repo-level: whether any remote is configured at all. Deliberately not the
    // branch's upstream - a merged branch whose remote-tracking ref has been
    // pruned still lives in a repo that has a remote.
    let remote_names = repo.remote_names();
    let remote = !remote_names.is_empty();
    let remote_url = remote_web_url_of(&repo, &remote_names);

    // Ahead/behind, in contrast, are branch-specific and need a tracking ref
    // that actually resolves to a commit to count against.
    let (mut ahead, mut behind) = (0, 0);

    if let (Some(name), Some(local)) = (head_name, head_id) {
        let local = local.detach();
        let upstream = repo
            .branch_remote_tracking_ref_name(name.as_ref(), gix::remote::Direction::Fetch)
            .and_then(Result::ok)
            .and_then(|tracking| repo.find_reference(tracking.as_bstr()).ok())
            .and_then(|mut reference| reference.peel_to_id().ok().map(|id| id.detach()));

        if let Some(upstream) = upstream {
            ahead = count_commits(&repo, local, upstream);
            behind = count_commits(&repo, upstream, local);
        }
    }

    GitStats {
        untracked,
        staged,
        non_staged,
        ahead,
        behind,
        conflicted,
        remote,
        remote_url,
        branch_name,
    }
}

/// Opens the repository containing `path`, searching upwards for it. The error
/// is boxed because gitoxide's is large enough to bloat every `Result` it
/// travels in.
fn discover(path: &Path) -> Result<gix::Repository, Box<gix::discover::Error>> {
    gix::ThreadSafeRepository::discover_opts(path, Default::default(), open_options())
        .map(Into::into)
        .map_err(Box::new)
}

/// Whether the repository at `path` has a remote, and the browser URL of the
/// preferred one - the two [`GitStats`] fields that need no status walk.
///
/// `None` when the repository cannot be opened, which is what lets the CLI
/// backend fall back to asking `git` (see [`super::process::remote_info`]).
pub(super) fn remote_info(path: &Path) -> Option<(bool, Option<String>)> {
    let repo = discover(path).ok()?;
    let names = repo.remote_names();
    Some((!names.is_empty(), remote_web_url_of(&repo, &names)))
}

/// Repository open options that drop the `gitattributes` file shipped with the
/// git installation.
///
/// Reading it is what [`gix::discover`] does by default, and on Windows the
/// only way gitoxide can find it is by running `git --exec-path` - a child it
/// spawns with `CREATE_NO_WINDOW`, costing around 55ms, four times the whole
/// status walk. What that buys is nothing the prompt can see: Git for Windows
/// ships an `etc/gitattributes` which does no more than map binary document
/// formats to the `astextplain` diff driver, shaping `git diff` output and
/// never the status walk.
///
/// The installation's `etc/gitconfig` does matter, and stays loaded:
/// [`super::preresolve_system_gitconfig`] hands gitoxide its path up front so
/// that file needs no probe either.
///
/// Only Windows is affected. Elsewhere the prefix is just `/`, costs no child
/// process, and `/etc/gitattributes` is a file git itself would read.
fn open_options() -> gix::sec::trust::Mapping<gix::open::Options> {
    use gix::sec::trust::DefaultForLevel;
    use gix::sec::Trust;

    let for_level = |level| {
        let mut options = gix::open::Options::default_for_level(level);
        if cfg!(windows) {
            options.permissions.attributes.system = false;
        }
        options
    };

    gix::sec::trust::Mapping {
        full: for_level(Trust::Full),
        reduced: for_level(Trust::Reduced),
    }
}

/// The browser URL of the preferred remote's fetch URL, if it has one.
fn remote_web_url_of(repo: &gix::Repository, names: &gix::remote::Names<'_>) -> Option<String> {
    let names: Vec<String> = names.iter().map(|name| name.to_string()).collect();
    let name = preferred_remote(names.iter().map(String::as_str))?;
    let remote = repo.find_remote(name).ok()?;
    let url = remote.url(gix::remote::Direction::Fetch)?;
    remote_web_url(&url.to_bstring().to_string())
}

/// The branch whose tip is `commit`, for labelling a detached HEAD. Local
/// branches are preferred; remote-tracking branches (`origin/main`) are only
/// consulted when no local branch matches.
fn branch_at(repo: &gix::Repository, commit: &gix::Id<'_>) -> Option<String> {
    let commit = commit.detach();
    let refs = repo.references().ok()?;
    for iter in [refs.local_branches().ok()?, refs.remote_branches().ok()?] {
        let found = preferred_branch(iter.flatten().filter_map(|reference| {
            // Symbolic refs such as `origin/HEAD` have no direct id and are skipped.
            (reference.try_id()?.detach() == commit).then(|| reference.name().shorten().to_string())
        }));
        if found.is_some() {
            return found;
        }
    }
    None
}

/// Whether an untracked directory entry is "hollow": a directory whose tree
/// contains no files at any depth (only empty subdirectories).
///
/// gitoxide's collapsing directory walk emits such a directory as a single
/// untracked entry, but `git status` (and the libgit2 / CLI backends) never
/// report a directory that holds no files. Without this filter the gitoxide
/// backend over-counts — e.g. a `screenshots/` containing only empty `new/` and
/// `reference/` subdirectories shows up as "1 new file" on an otherwise clean
/// worktree.
///
/// A plain filesystem check is sufficient here: gitoxide only classifies a
/// directory as untracked when it has untracked file content, so a directory
/// that holds solely ignored files is already reported as clean and never
/// reaches this code. Thus "has any file on disk" exactly distinguishes a real
/// untracked directory from a hollow tree of empty directories.
fn is_hollow_untracked_dir(repo: &gix::Repository, entry: &gix::dir::Entry) -> bool {
    if entry.disk_kind != Some(gix::dir::entry::Kind::Directory) {
        return false;
    }
    match repo.workdir_path(entry.rela_path.clone()) {
        Some(path) => !dir_contains_file(&path),
        None => false,
    }
}

/// Returns `true` as soon as a non-directory entry (a file, symlink, etc.) is
/// found anywhere beneath `dir`, short-circuiting on the first hit. Recursion
/// only descends through subdirectories, so the cost is bounded by the number of
/// empty directories — a directory with files returns almost immediately. An
/// unreadable directory is treated as containing no files.
fn dir_contains_file(dir: &Path) -> bool {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return false;
    };
    for entry in entries.flatten() {
        match entry.file_type() {
            // `file_type()` does not follow symlinks, so a symlinked directory
            // is treated as a (non-directory) entry and cannot cause a cycle.
            Ok(kind) if kind.is_dir() => {
                if dir_contains_file(&entry.path()) {
                    return true;
                }
            }
            Ok(_) => return true,
            Err(_) => continue,
        }
    }
    false
}

/// Count commits reachable from `tip` but not from `excluded`, i.e. the
/// `excluded..tip` range — equivalent to `git rev-list --count excluded..tip`.
fn count_commits(repo: &gix::Repository, tip: gix::ObjectId, excluded: gix::ObjectId) -> u32 {
    match repo.rev_walk(Some(tip)).with_hidden(Some(excluded)).all() {
        Ok(walk) => walk.filter(Result::is_ok).count() as u32,
        Err(_) => 0,
    }
}

#[cfg(test)]
mod tests {
    use std::path::{Path, PathBuf};
    use std::process::Command;
    use std::sync::atomic::{AtomicU32, Ordering};

    use super::{dir_contains_file, remote_info, run_git};

    fn unique_temp_dir() -> PathBuf {
        static COUNTER: AtomicU32 = AtomicU32::new(0);
        let n = COUNTER.fetch_add(1, Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!("superline-gix-{}-{n}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn git(repo: &Path, args: &[&str]) {
        let status = Command::new("git")
            .current_dir(repo)
            .args(args)
            .status()
            .unwrap();
        assert!(status.success(), "`git {}` failed", args.join(" "));
    }

    fn init_repo() -> PathBuf {
        let dir = unique_temp_dir();
        git(&dir, &["init", "-q", "-b", "main"]);
        git(&dir, &["config", "user.email", "test@example.com"]);
        git(&dir, &["config", "user.name", "test"]);
        git(&dir, &["commit", "-q", "--allow-empty", "-m", "init"]);
        dir
    }

    /// The label both backends should give HEAD when detached at `branch`.
    fn detached(repo: &Path, branch: &str) -> String {
        let output = Command::new("git")
            .current_dir(repo)
            .args(["rev-parse", "--short", "HEAD"])
            .output()
            .unwrap();
        let hash = String::from_utf8(output.stdout).unwrap();
        super::detached_label(Some(branch.to_owned()), hash.trim())
    }

    /// The CLI backend takes its remote fields from here rather than spending a
    /// `git remote` spawn on them, so they have to agree with what a status
    /// walk of the same repository reports.
    #[test]
    fn remote_info_matches_the_full_status_walk() {
        let repo = init_repo();
        assert_eq!(remote_info(&repo), Some((false, None)));

        git(
            &repo,
            &["remote", "add", "upstream", "https://example.com/them.git"],
        );
        git(
            &repo,
            &["remote", "add", "origin", "git@github.com:me/mine.git"],
        );

        let stats = run_git(&repo);
        assert_eq!(
            remote_info(&repo),
            Some((stats.remote, stats.remote_url.clone()))
        );
        // `origin` wins over `upstream`, and the scp-style URL becomes a page.
        assert_eq!(
            stats.remote_url.as_deref(),
            Some("https://github.com/me/mine")
        );

        std::fs::remove_dir_all(&repo).ok();
    }

    /// Regression test: gitoxide's collapsing walk emits an untracked directory
    /// even when it contains nothing but empty subdirectories, where `git
    /// status` reports a clean tree. Such a directory must not be counted.
    #[test]
    fn untracked_dir_of_only_empty_subdirs_is_not_counted() {
        let repo = init_repo();
        std::fs::create_dir_all(repo.join("screenshots/new")).unwrap();
        std::fs::create_dir_all(repo.join("screenshots/reference")).unwrap();

        assert_eq!(
            run_git(&repo).untracked,
            0,
            "a directory holding only empty subdirectories must not count as untracked"
        );

        // A real file anywhere in the tree makes it a genuine untracked
        // directory, which git collapses into a single entry.
        std::fs::write(repo.join("screenshots/new/shot.png"), b"x").unwrap();
        assert_eq!(
            run_git(&repo).untracked,
            1,
            "an untracked directory containing a file counts once"
        );

        std::fs::remove_dir_all(&repo).ok();
    }

    #[test]
    fn detached_head_at_a_branch_tip_names_the_branch() {
        let repo = init_repo();
        git(&repo, &["checkout", "-q", "--detach"]);

        let expected = detached(&repo, "main");
        assert_eq!(run_git(&repo).branch_name, expected);
        assert_eq!(super::super::process::run_git(&repo).branch_name, expected);

        std::fs::remove_dir_all(&repo).ok();
    }

    #[test]
    fn detached_head_off_every_branch_tip_shows_only_the_hash() {
        let repo = init_repo();
        git(&repo, &["checkout", "-q", "--detach"]);
        git(&repo, &["commit", "-q", "--allow-empty", "-m", "adrift"]);

        let stats = run_git(&repo);
        assert!(
            !stats.branch_name.contains(' ') && stats.branch_name.len() >= 7,
            "expected a bare hash, got {:?}",
            stats.branch_name
        );

        std::fs::remove_dir_all(&repo).ok();
    }

    #[test]
    fn detached_head_at_a_remote_tip_falls_back_to_the_remote_branch() {
        let repo = init_repo();
        git(&repo, &["update-ref", "refs/remotes/origin/main", "HEAD"]);
        git(
            &repo,
            &[
                "symbolic-ref",
                "refs/remotes/origin/HEAD",
                "refs/remotes/origin/main",
            ],
        );
        // Move `main` on so only the remote-tracking ref matches the old tip.
        git(&repo, &["commit", "-q", "--allow-empty", "-m", "ahead"]);
        git(
            &repo,
            &["checkout", "-q", "--detach", "refs/remotes/origin/main"],
        );

        let expected = detached(&repo, "origin/main");
        assert_eq!(run_git(&repo).branch_name, expected);
        assert_eq!(super::super::process::run_git(&repo).branch_name, expected);

        std::fs::remove_dir_all(&repo).ok();
    }

    #[test]
    fn dir_contains_file_finds_nested_files_only() {
        let dir = unique_temp_dir();
        std::fs::create_dir_all(dir.join("a/b/c")).unwrap();
        assert!(
            !dir_contains_file(&dir),
            "a tree of empty dirs has no files"
        );

        std::fs::write(dir.join("a/b/c/leaf"), b"x").unwrap();
        assert!(dir_contains_file(&dir), "a nested file must be found");

        std::fs::remove_dir_all(&dir).ok();
    }
}
