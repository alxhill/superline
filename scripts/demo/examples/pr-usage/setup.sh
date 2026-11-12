# shellcheck shell=bash
# Runs in the demo's working directory with scripts/lib/vhs-scene.sh sourced:
# a repo on feat/login, one commit ahead of its upstream, with local changes.
git_repo_with_upstream "$PWD" 1 0 feat/login
echo "## Login" >>README.md
touch login.rs
