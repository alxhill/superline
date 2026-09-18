# Repository instructions

- Do not update `CHANGELOG.md` manually. It is managed by Release Please.
- Add the `autorelease` label to a PR to ship it as a new version as soon as
  it lands: release-plz's release PR is then queued to merge once CI is green.
  Without the label the release PR stays open until merged by hand.
- When adding a new widget, add it to `Config::default()` in `src/config.rs` so
  a fresh install shows it.
- When a widget needs a slow lookup (network, big directory walk), implement
  `cache::Source` in `src/cache.rs` for it, render through `Cached::load` (or
  `load_with_timeout`), and register the type in `modules::run_refresh`. Do not
  hand-roll cache files or background processes.
- Once a change is ready, run `cargo install --path .` so it is available for
  manual testing.
- Run `superline clear-caches` to wipe all cached data (git status, PR lookups,
  AI usage) under `<cache_dir>/superline/` when testing the async refresh paths
  from a cold start.
- Default to creating a new branch and PR at the start of each session.
- When a change adds a new feature or option, also update the user's live
  superline config (`~/.config/superline/config.json`, or the file it points at)
  to use it so it can be tried immediately. If that config lives in a git repo,
  edit it in place but do not commit it.
- If a prompt starts with "ONESHOT", perform the full branch -> PR -> CI -> merge cycle without requesting further input, unless blocked.
