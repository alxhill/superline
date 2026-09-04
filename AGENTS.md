# Repository instructions

- Do not update `CHANGELOG.md` manually. It is managed by Release Please.
- When adding a new widget, add it to `Config::default()` in `src/config.rs` so
  a fresh install shows it.
- Once a change is ready, run `cargo install --path .` so it is available for
  manual testing.
