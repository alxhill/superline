# Terminal screenshot tests

The terminal snapshot rig launches the branch-local `superline` binary inside
real interactive shells and photographs the prompt with
[VHS](https://github.com/charmbracelet/vhs). VHS runs each shell through
[ttyd](https://github.com/tsl0922/ttyd) on the native pseudoterminal (PTY on
Unix, ConPTY on Windows), renders it with xterm.js in headless Chromium, and
writes PNG screenshots. No desktop session or visible terminal window is
required, and no terminal emulation lives in this repository.

Each shell produces:

- `<platform>-<shell>-clean.png`: the first prompt in the fixture directory.
- `<platform>-<shell>-failure.png`: the prompt after a command exits with
  status 7, with the previous prompt still intact above it.
- `<platform>-<shell>.tape` and `<platform>-<shell>.log`: the generated VHS
  tape and VHS's output, for diagnosing a failed capture.

The fixture uses a fixed 100x12 terminal, a pinned font, theme, and locale, an
isolated home directory, and a deterministic config, so captures from
different platforms are directly comparable.

## Dependencies

These are test-only dependencies; nothing here is needed to use superline.

- The shells to exercise: `bash`, `zsh`, `fish`, `pwsh`, `nu`.
- `ttyd` and `ffmpeg` on `PATH` (`brew install ttyd ffmpeg` on macOS; on
  Windows the CI workflow downloads pinned builds).
- Chrome or Chromium. VHS downloads a Chromium into its cache if none is found.
- The `MesloLGS Nerd Font` family installed for the OS, so Chromium can
  resolve it by name. Download `MesloLGSNerdFont-Regular.ttf` from the
  [Nerd Fonts release](https://github.com/ryanoasis/nerd-fonts/releases/tag/v3.5.1)
  and install it like any other font.
- Go, to build the pinned VHS:

  ```bash
  examples/terminal-snapshot/build-vhs.sh target/vhs-bin
  ```

  The script checks out the VHS v0.12.0 commit and applies
  `examples/terminal-snapshot/vhs-fixes.patch`. The release binary renders
  its screenshots with an already-cancelled context and writes nothing
  ([charmbracelet/vhs#787](https://github.com/charmbracelet/vhs/issues/787)),
  and it starts ttyd without a working directory, which ttyd's Windows build
  needs before it can spawn the shell
  ([tsl0922/ttyd#1413](https://github.com/tsl0922/ttyd/issues/1413)). The
  patch also resolves the shell through `PATH`, because Windows'
  `CreateProcess` searches System32 first and would start the WSL `bash` stub
  instead of Git Bash. The earlier VHS v0.11.0 hangs on Windows with current
  Chrome. Once releases carry the fixes, drop the patch and use the release
  binary directly.

## Run locally

```bash
cargo build --bin superline --example terminal-snapshot
cargo run --example terminal-snapshot -- \
  --shell all \
  --scenario all \
  --vhs target/vhs-bin/vhs \
  --output target/terminal-snapshots
```

The rig always uses the `superline` binary from the same target directory, so
there is no need to `cargo install` it. Missing shells are reported and
skipped; add `--require-all` when a missing shell should fail the run.
`SUPERLINE_E2E_VHS` can be used instead of `--vhs`, and without either the rig
looks for `vhs` on `PATH`.

To debug a prompt from a real config, capture it instead of the fixture config
and run the shell inside an existing directory (for example a Cargo project,
so directory-aware widgets have something to show). Under `--config` the tape
only asserts on the `cmd` widget's success chevron and failure status, since
the surrounding layout is whatever that config renders:

```bash
cargo run --example terminal-snapshot -- \
  --shell zsh \
  --config ~/.config/superline/config.json \
  --workdir "$PWD" \
  --output target/prompt-debug \
  --vhs target/vhs-bin/vhs
```

A theme file the config names relative to itself is copied along with it.
Widgets with background lookups (for example `ai_usage`) start cold in the
isolated home; remove them from a copy of the config if their probes are
unwanted.

Select individual shells or scenarios by repeating the corresponding flag:

```bash
cargo run --example terminal-snapshot -- \
  --shell zsh --shell pwsh \
  --scenario failure \
  --vhs target/vhs-bin/vhs
```

## How a capture works

`examples/terminal-snapshot.rs` is a thin orchestrator. For each shell it:

1. creates a scratch home with `examples/terminal-snapshot/config.json` and a
   `superline-e2e` working directory;
2. runs `superline init <shell>` and saves the snippet as a startup file
   (PowerShell's copy also gets an explicit `--config` path);
3. fills `examples/terminal-snapshot/tape.template` and writes the tape next
   to the screenshots;
4. runs VHS with a pinned locale and `PATH` pointed at the branch-local
   binary;
5. fails unless every expected PNG was written.

The tape hides the setup, waits for the shell's stock prompt, types one line
that exports the isolated home (`HOME`, `USERPROFILE`, `XDG_CONFIG_HOME`,
`XDG_CACHE_HOME`), sources the snippet, enters the fixture directory, and
clears the screen, then shows the recording. Paths are typed with forward
slashes, which PowerShell, nushell, and Git Bash all accept on Windows. The home is exported inside the
shell rather than on the VHS process because Chrome on Windows resolves its
own app-data folders through `%USERPROFILE%` and exits when that points at
the fixture. Every screenshot is gated on `Wait+Screen`
assertions: the clean prompt must show the success chevron, and the failure
prompt must show `7` while the clean prompt and the typed command are still
visible above it. A capture whose prompt was overwritten or never rendered
fails instead of producing a misleading image.

## CI artifacts

The `Terminal snapshots` workflow captures every supported shell on macOS, and
PowerShell and Git Bash through ConPTY on Windows. It builds the pinned VHS, installs
pinned, checksum-verified ttyd, ffmpeg (Windows), and Nerd Font builds, and
uploads `terminal-snapshots-macos` and `terminal-snapshots-windows` artifacts
on success or failure. They are retained for 14 days on every pull request,
`main` push, and manual run.

The screenshots deliberately remain build artifacts instead of committed
goldens. Shell versions and font rendering vary between runner images; the PNG
is intended for human visual review while prompt content and escape-style
contracts remain covered by the normal Rust tests.

## What this does and does not prove

The rig exercises shell startup snippets, each shell's prompt hook, terminal
width calculation, ANSI color, Nerd Font glyphs, multi-row layout, right
prompts, exit-status propagation, and that a new prompt does not clobber the
previous one. Everything is rendered and answered by a pinned xterm.js-based
terminal, so it does not guarantee identical behavior in every native terminal
application. Host-specific settings and timing-sensitive input such as rapid
Ctrl-C still need a focused manual run in that terminal.
