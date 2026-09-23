# Terminal snapshot fixtures

Support files for `examples/terminal-snapshot.rs`, which captures superline
prompts from real interactive shells through [VHS](https://github.com/charmbracelet/vhs).
See [docs/terminal-snapshots.md](../../docs/terminal-snapshots.md) for usage.

- `cases.json`: the built-in cases (config, terminal size, steps, and checks).
- `config.json`: deterministic superline config for cases that do not set one.
- `tape.template`: shared VHS settings and setup; the example fills in the
  shell, size, and steps per run.
- `build-vhs.sh`: builds the pinned VHS commit with `vhs-fixes.patch`.
- `vhs-fixes.patch`: fixes for VHS v0.12.0: render with a live context so
  screenshots are written (charmbracelet/vhs#787), pass ttyd a working
  directory so its Windows build can spawn the shell (tsl0922/ttyd#1413),
  resolve the shell through `PATH` so Windows starts Git Bash rather than the
  WSL stub in System32, and read the visible viewport rather than the top of
  the scrollback for screen text. Drop it once releases include these.
