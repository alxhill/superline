# Terminal snapshot rig

Runs superline in real interactive shells through
[VHS](https://github.com/charmbracelet/vhs). See
[docs/terminal-snapshots.md](../../docs/terminal-snapshots.md) for usage.

- `cases/<name>/`: one directory per case, with a superline `config.json` and
  a VHS `case.tape`. The checks live in `tests/terminal_snapshots.rs`.
- `rig.rs`: fixtures, tape generation, VHS runs, and snapshot text, shared by
  the test and `examples/terminal-snapshot.rs`.
- `config.json`: the config for cases that do not bring their own.
- `tape.template`: shared VHS settings and the hidden shell setup.
- `build-vhs.sh`: builds the pinned VHS commit with `vhs-fixes.patch`.
- `vhs-fixes.patch`: fixes for VHS v0.12.0: render with a live context so
  screenshots are written (charmbracelet/vhs#787), pass ttyd a working
  directory so its Windows build can spawn the shell (tsl0922/ttyd#1413),
  resolve the shell through `PATH` so Windows starts Git Bash rather than the
  WSL stub in System32, and read the visible viewport rather than the top of
  the scrollback for screen text. Drop it once releases include these.
