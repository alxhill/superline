# Terminal snapshot fixtures

Support files for `examples/terminal-snapshot.rs`, which captures superline
prompts from real interactive shells through [VHS](https://github.com/charmbracelet/vhs).
See [docs/terminal-snapshots.md](../../docs/terminal-snapshots.md) for usage.

- `config.json`: deterministic superline config written into each isolated home.
- `tape.template`: shared VHS settings and capture flow; the example fills the
  placeholders per shell.
- `build-vhs.sh`: builds the pinned VHS commit with `vhs-render-context.patch`.
- `vhs-render-context.patch`: upstream fix for VHS v0.12.0 writing no output
  (charmbracelet/vhs#787). Drop it once a release includes the fix.
