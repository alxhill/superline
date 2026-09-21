# Terminal screenshot tests

The terminal snapshot rig launches the real `superline` binary inside real
interactive shells, through a native pseudoterminal (PTY on Unix, ConPTY on
Windows). It parses the resulting VT stream into a terminal cell grid and
rasterizes that grid to PNG with a Nerd Font. No desktop session or visible
terminal window is required.

This gives each capture three useful layers:

- `.png` is the review and documentation artifact.
- `.txt` makes missing prompt content easy to diagnose in CI logs.
- `.ansi` preserves the original terminal stream for low-level debugging or
  replay.

The built-in `clean` and `failure` scenarios verify both normal rendering and
the red exit-status prompt after a real failing command. The fixture uses a
fixed 100-column terminal, an isolated home directory, and a deterministic
config so screenshots from different platforms are directly comparable.

## Run locally

Install the shells you want to exercise and locate a `.ttf` Nerd Font. Then:

```bash
cargo build --bin superline --example terminal-snapshot
cargo run --example terminal-snapshot -- \
  --shell all \
  --scenario all \
  --font "$HOME/Library/Fonts/MesloLGSNerdFont-Regular.ttf" \
  --output target/terminal-snapshots
```

Missing shells are reported and skipped. Add `--require-all` when a missing
requested shell should fail the run. `SUPERLINE_E2E_FONT` can be used instead
of `--font`.

Select individual shells or scenarios by repeating the corresponding flag:

```bash
cargo run --example terminal-snapshot -- \
  --shell zsh --shell pwsh \
  --scenario failure \
  --font /path/to/NerdFont.ttf
```

The harness supports `bash`, `zsh`, `fish`, `pwsh`, and `nu`.

## CI artifacts

The `Terminal snapshots` workflow captures every supported shell on macOS and
PowerShell through native ConPTY on Windows. Its `terminal-snapshots-macos` and
`terminal-snapshots-windows` artifacts are retained for 14 days on every pull
request, `main` push, and manual run. The workflow downloads a version-pinned,
checksum-verified Meslo Nerd Font so glyph rasterization is reproducible.

The screenshots deliberately remain build artifacts instead of committed
goldens. Font rasterization and shell versions vary between runner images; the
PNG is intended for human visual review while prompt content and escape-style
contracts remain covered by the normal Rust tests.

## What this does and does not prove

The rig exercises shell startup files, each shell's prompt hook, terminal width
calculation, ANSI color, Nerd Font glyphs, multi-row layout, right prompts, and
exit-status propagation. The Windows job is a real interactive PowerShell
session backed by ConPTY, so it covers substantially more than invoking the
`prompt` function non-interactively.

It does not create a Windows Terminal or iTerm2 GUI window. Host-specific
settings and timing-sensitive input such as rapid Ctrl-C still need a focused
manual run in that terminal. The PTY driver is intentionally kept in one
example so additional input scenarios can be added without changing the
shipping binary.
