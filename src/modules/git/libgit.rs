use std::path::Path;

use git2::{Branch, BranchType, ObjectType, Repository, Status, StatusOptions, StatusShow};

use super::GitStats;

pub fn run_git(path: &Path) -> GitStats {
    let repository = Repository::open(path).unwrap();

    let mut status_options = StatusOptions::new();
    status_options
        .show(StatusShow::IndexAndWorkdir)
        .include_untracked(true)
        .renames_from_rewrites(true)
        .renames_head_to_index(true);

    let remote = has_remote(&repository);

    let (mut untracked, mut non_staged, mut conflicted, mut staged, mut ahead, mut behind) =
        (0, 0, 0, 0, 0, 0);

    for status in repository
        .statuses(Some(&mut status_options))
        .unwrap()
        .iter()
        .map(|ref x| x.status())
    {
        if status.intersects(
            Status::INDEX_NEW
                | Status::INDEX_MODIFIED
                | Status::INDEX_TYPECHANGE
                | Status::INDEX_RENAMED
                | Status::INDEX_DELETED,
        ) {
            staged += 1;
        }
        if status.is_wt_new() {
            untracked += 1;
        }
        if status.is_conflicted() {
            conflicted += 1;
        }
        if status.intersects(Status::WT_MODIFIED | Status::WT_TYPECHANGE | Status::WT_DELETED) {
            non_staged += 1;
        }
    }

    let active_branch: Option<Branch> = repository
        .branches(Some(BranchType::Local))
        .unwrap()
        .filter_map(Result::ok)
        .map(|x| x.0)
        .find(|b| b.is_head());

    if let Some(ref active_branch) = active_branch {
        let local = active_branch.get().target();
        let upstream = active_branch
            .upstream()
            .ok()
            .and_then(|obj| obj.get().target());

        // Ahead/behind are branch-specific and need a tracking ref that
        // actually resolves to a commit to count against.
        if let (Some(local), Some(upstream)) = (local, upstream) {
            let (a, b) = repository.graph_ahead_behind(local, upstream).unwrap();
            ahead = a as u32;
            behind = b as u32;
        };
    }

    let branch_name = active_branch
        .as_ref()
        .and_then(|x| x.name().unwrap())
        .map(ToOwned::to_owned)
        .unwrap_or_else(|| {
            if let Ok(head) = repository.head() {
                let target = head.target().unwrap();

                repository
                    .find_object(target, Some(ObjectType::Any))
                    .unwrap()
                    .short_id()
                    .unwrap()
                    .as_str()
                    .unwrap()
                    .to_owned()
            } else {
                String::from("Big Bang")
            }
        });

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

/// Whether the repository has any remote configured. Repo-level rather than
/// asking whether the current branch's upstream resolves: a branch that was
/// never pushed, or one whose remote-tracking ref has been pruned after the
/// remote branch was deleted, still lives in a repo that has a remote.
fn has_remote(repository: &Repository) -> bool {
    repository.remotes().is_ok_and(|names| !names.is_empty())
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::sync::atomic::{AtomicU32, Ordering};

    use git2::Repository;

    use super::has_remote;

    fn temp_repo() -> Repository {
        static COUNTER: AtomicU32 = AtomicU32::new(0);
        let n = COUNTER.fetch_add(1, Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!("superline-libgit-{}-{n}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        Repository::init(&dir).unwrap()
    }

    #[test]
    fn a_repo_without_any_remote_has_none() {
        assert!(!has_remote(&temp_repo()));
    }

    #[test]
    fn a_configured_remote_counts_even_with_no_branch_pushed() {
        let repo = temp_repo();
        repo.remote("origin", "https://example.invalid/repo.git")
            .unwrap();

        assert!(has_remote(&repo));
    }
}
