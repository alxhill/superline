use std::path::Path;

use gix::status::index_worktree::Item as IndexWorktreeItem;
use gix::status::plumbing::index_as_worktree::EntryStatus;
use gix::status::Item;

use super::GitStats;

/// gitoxide (pure-Rust) git backend. Produces the same [`GitStats`] the libgit
/// and CLI backends do: a count of staged / non-staged / untracked / conflicted
/// paths, whether the repository has a remote configured, and the ahead/behind
/// distance from the upstream tracking branch.
pub fn run_git(path: &Path) -> GitStats {
    let repo = gix::discover(path).unwrap();

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
        // Detached HEAD: show the short commit hash.
        (None, Some(id)) => id.shorten_or_id().to_string(),
        // Unborn branch / no HEAD: match the libgit & CLI "Big Bang" label.
        _ => String::from("Big Bang"),
    };

    // Repo-level: whether any remote is configured at all. Deliberately not the
    // branch's upstream - a merged branch whose remote-tracking ref has been
    // pruned still lives in a repo that has a remote.
    let remote = !repo.remote_names().is_empty();

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
        branch_name,
    }
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

    use super::{dir_contains_file, run_git};

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
