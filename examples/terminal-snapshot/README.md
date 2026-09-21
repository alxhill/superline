# Terminal snapshot fixtures

Support files for `examples/terminal-snapshot.rs`, which captures superline
prompts from real interactive shells through [VHS](https://github.com/charmbracelet/vhs).
See [docs/terminal-snapshots.md](../../docs/terminal-snapshots.md) for usage.

- `config.json`: deterministic superline config written into each isolated home.
- `tape.template`: shared VHS settings and capture flow; the example fills the
  placeholders per shell.
- `build-vhs.sh`: builds the pinned VHS commit with `vhs-fixes.patch`.
- `vhs-fixes.patch`: fixes for VHS v0.12.0: render with a live context so
  screenshots are written (charmbracelet/vhs#787), pass ttyd a working
  directory so its Windows build can spawn the shell (tsl0922/ttyd#1413), and
  resolve the shell through `PATH` so Windows starts Git Bash rather than the
  WSL stub in System32. Drop it once releases include these.
