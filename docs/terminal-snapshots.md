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
different platforms are directly comparable. The grid is rendered at a 36px
font, so the resulting PNGs are 2x density and display crisply on high-DPI
screens.

## Dependencies

These are test-only dependencies; nothing here is needed to use superline.

- The shells to exercise: `bash`, `zsh`, `fish`, `pwsh`, `nu`. The
  `bash-3.2` variant runs macOS's `/bin/bash` (bash 3.2) through a shim on
  `PATH`, next to the `bash` found on `PATH` (Homebrew's bash 5 in CI), and is
  skipped where `/bin/bash` is not bash 3.x.
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
2. for bash, zsh, and fish, runs `superline install <shell>` against the
   scratch home, so the capture loads the exact line users get in `.bashrc`,
   `.zshrc`, or `config.fish`. PowerShell and nushell resolve their profile
   paths outside the home directory, so for them it saves `superline init
   <shell>` output as a startup file instead (PowerShell's copy also gets an
   explicit `--config` path);
3. fills `examples/terminal-snapshot/tape.template` and writes the tape next
   to the screenshots;
4. runs VHS with a pinned locale and `PATH` pointed at the branch-local
   binary;
5. fails unless every expected PNG was written.

The tape hides the setup, waits for the shell's stock prompt, types one line
that exports the isolated home (`HOME`, `USERPROFILE`, `XDG_CONFIG_HOME`,
`XDG_CACHE_HOME`), sources the startup file, enters the fixture directory, and
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

The `Terminal snapshots` workflow captures every supported shell on macOS,
including both bash 5 and the system bash 3.2, and
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

## Website screenshots

The screenshots on the [website](https://alxhill.github.io/superline/) come
from the same VHS build, driven by `scripts/site-screenshots/generate.sh`
rather than the test rig. Each scene in `scripts/site-screenshots/scenes.sh`
builds a fixture (git repos with an upstream, language projects, a seeded AI
usage cache) in an isolated home, runs a short fish session with its config
from `scripts/site-screenshots/configs/`, and writes a trimmed PNG to
`site/img/`. A `gh` stub in `scripts/site-screenshots/bin/` answers PR lookups,
so no network access or GitHub login is needed.

```bash
scripts/site-screenshots/generate.sh          # every scene
scripts/site-screenshots/generate.sh pr usage # just these
```

The [configuration reference](https://alxhill.github.io/superline/config.html)
is driven by `scripts/site-screenshots/components.json`. Each entry lists a
component's example variants: the row (or whole config) to render, the fixture
to render it in, and any commands to type first. The `components` scene
captures every variant to `site/img/config/`, and `render_examples.py` fills
the matching `<div class="example" data-example="<component>/<variant>">`
placeholders in `site/config.html` with the screenshot and the JSON it came
from. To add an example, add a variant to the manifest and a placeholder to
the page, then run:

```bash
COMPONENTS="git" scripts/site-screenshots/generate.sh components
```

Each component's collapsible *Theme options* table comes from
`scripts/site-screenshots/theme-options.json`, which mirrors the properties
`src/themes/custom.rs` reads. `render_examples.py` fills every
`<details class="theme-opts" data-theme="<module>">` placeholder from it; run
it directly (`uv run --with pillow python scripts/site-screenshots/render_examples.py`)
after editing the file, since no screenshots are involved.

Besides the dependencies above it needs `fish`, `jq`, `uv`, and
[Symbols Nerd Font Mono](https://github.com/ryanoasis/nerd-fonts/releases/tag/v3.5.1)
(`NerdFontsSymbolsOnly.zip`) 3.4 or newer, which supplies the Claude and Codex
glyphs when the installed Meslo predates them. `trim.py` crops each capture
and opens a 10px gap between terminal rows, so stacked prompts do not run
together; it finds the row boundaries from the long horizontal edges of the
segments, since VHS scales its screenshots. The script also updates the
image sizes on both pages. Commit the regenerated PNGs; the `Website`
workflow publishes `site/` to GitHub Pages on every push to `main` that
touches it.
