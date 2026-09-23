# Terminal screenshot tests

The terminal snapshot rig launches the branch-local `superline` binary inside
real interactive shells and photographs the prompt with
[VHS](https://github.com/charmbracelet/vhs). VHS runs each shell through
[ttyd](https://github.com/tsl0922/ttyd) on the native pseudoterminal (PTY on
Unix, ConPTY on Windows), renders it with xterm.js in headless Chromium, and
writes PNG screenshots. No desktop session or visible terminal window is
required, and no terminal emulation lives in this repository.

Every capture is a *case*: a plain superline `config.json` and a plain VHS
tape in a directory under [`tests/terminal/cases/`](../tests/terminal/cases),
run in each shell at one or more terminal widths. Each `Screenshot` in the tape
produces a PNG and the visible screen text at the same moment, and
[`tests/terminal_snapshots.rs`](../tests/terminal_snapshots.rs) checks that
text in Rust. Every capture uses a pinned font, theme, and locale and an
isolated home directory, so captures from different platforms are directly
comparable. The grid is rendered at a 36px font, so the resulting PNGs are 2x
density and display crisply on high-DPI screens.

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
  tests/terminal/build-vhs.sh target/vhs-bin
  ```

  The script checks out the VHS v0.12.0 commit and applies
  `tests/terminal/vhs-fixes.patch`. The release binary renders
  its screenshots with an already-cancelled context and writes nothing
  ([charmbracelet/vhs#787](https://github.com/charmbracelet/vhs/issues/787)),
  and it starts ttyd without a working directory, which ttyd's Windows build
  needs before it can spawn the shell
  ([tsl0922/ttyd#1413](https://github.com/tsl0922/ttyd/issues/1413)). The
  patch also resolves the shell through `PATH`, because Windows'
  `CreateProcess` searches System32 first and would start the WSL `bash` stub
  instead of Git Bash, and makes the screen text VHS reads for `Wait+Screen`
  and its text output start at the visible viewport rather than the top of
  the scrollback. The earlier VHS v0.11.0 hangs on Windows with current
  Chrome. Once releases carry the fixes, drop the patch and use the release
  binary directly.

## Run the tests

```bash
SUPERLINE_E2E_VHS=target/vhs-bin/vhs cargo test --test terminal_snapshots
```

Without `SUPERLINE_E2E_VHS` the test reports itself skipped, so a plain
`cargo test` does not need any of the dependencies above. It uses the
`superline` binary Cargo builds for the test, so there is no need to
`cargo install` it. Pass test names after `--` to run only those
(`cargo test --test terminal_snapshots -- multiline widths`). These variables
tune a run:

| Variable | Effect |
| --- | --- |
| `SUPERLINE_E2E_SHELLS` | Shells to run, comma-separated (default `all`). Missing shells are skipped. |
| `SUPERLINE_E2E_REQUIRE_ALL=1` | Fail instead of skipping a missing shell. |
| `SUPERLINE_E2E_OUTPUT` | Output directory (default `target/terminal-snapshots`). |
| `SUPERLINE_E2E_JOBS` | Captures to run at once (default 2). |

Each case gets a directory in the output, with, per shell and size:

- `<platform>-<shell>-<cols>x<rows>-<name>.png` and `.txt`: each screenshot
  and the visible screen text at the same moment.
- `<platform>-<shell>-<cols>x<rows>.tape`, `.log`, and `.frames.txt`: the
  full generated tape, VHS's output, and every screen dump VHS recorded, for
  diagnosing a failed capture.

A failing check prints its message and the screen text it was looking at.

## One-off captures

The `terminal-snapshot` example runs a config (and optionally a tape) without
any checks and prints each snapshot's screen text, which is handy for
debugging a prompt from a real config:

```bash
cargo build --bin superline --example terminal-snapshot
cargo run --example terminal-snapshot -- \
  --shell zsh \
  --config ~/.config/superline/config.json \
  --tape my-steps.tape \
  --workdir "$PWD" \
  --columns 80 --columns 140 \
  --vhs target/vhs-bin/vhs
```

Without `--tape` it takes one snapshot of the first prompt; `--case <name>`
uses a test case's config and tape instead. `--rows`, `--env KEY=VALUE`,
`--output`, and `--jobs` work as their test equivalents do; see `--help`. A
theme file the config names relative to itself is copied along with it.
Widgets with background lookups (for example `ai_usage`) start cold in the
isolated home; remove them from a copy of the config if their probes are
unwanted.

## Writing a case

A case is a directory under `tests/terminal/cases/`:

- `config.json`: the superline config. Without one, the shared
  `tests/terminal/config.json` is used. Any other file in the directory (a
  theme, say) is copied next to it.
- `case.tape`: VHS commands to run once the first prompt has started drawing.
  Without one, the case takes a single `prompt.png` snapshot.

The tape is ordinary [VHS](https://github.com/charmbracelet/vhs#vhs-command-reference)
(`Type`, `Enter`, `Wait+Screen /regex/`, `Sleep`, `Ctrl+C`, ...) with a few
rules. The rig owns the settings and the setup, so `Set`, `Output`, `Hide`,
`Show`, `Require`, and `Source` are rejected; terminal sizes and the fixture
are set in code. `Screenshot <name>.png` takes a named snapshot, and the rig
adds a short pause around each one. A tape is shared by every shell, so for
things shells spell differently it can call `sl-test`, a helper the rig puts on
`PATH`: `sl-test exit <N>` exits with status N, and `sl-test print <TEXT>`
prints without a trailing newline. Wait for the screen to settle before each
screenshot: `Wait+Screen /\x{F105}/` waits for the `cmd` widget's success mark.

```text
Wait+Screen /\x{F105}/
Screenshot first.png
Type "echo hello"
Enter
Wait+Screen /\nhello\n(?s:.*)\x{F105}/
Screenshot second.png
```

Then add an entry to `TESTS` in `tests/terminal_snapshots.rs` with the terminal
sizes, an optional working directory (relative to the scratch root, created if
missing), and a check function. The check receives a `Capture` with the shell
and size, and `capture.snapshot("first")` returns the screen text and PNG path.
`Snapshot::check` fails with the screen text attached, and `rig::width` gives a
line's width in terminal columns. The `SEP`, `ROUND`, and `OK` constants are
the separator glyphs and the success mark. Right sides on the last row are
drawn by fish, zsh, and nushell but not bash or PowerShell
(`Shell::draws_last_row_right`), and zsh leaves one column free to the right of
its right prompt.

## How a capture works

`tests/terminal/rig.rs` does the work for both the test and the example. For
each case, shell, and width it:

1. creates a scratch home with the case's config files and working directory;
2. runs `superline init <shell>` and saves the snippet as a startup file
   (PowerShell's copy also gets an explicit `--config` path);
3. fills `tests/terminal/tape.template` with the shell, size, and setup,
   appends the case's tape, and writes the result next to the screenshots;
4. runs VHS with a pinned locale and `PATH` pointed at the branch-local
   binary and the `sl-test` helper (a link to the running executable);
5. pairs each `Screenshot` with the screen dump VHS recorded after it and
   saves that as the snapshot's text.

The tape hides the setup, waits for the shell's stock prompt, types one line
that exports the isolated home (`HOME`, `USERPROFILE`, `XDG_CONFIG_HOME`,
`XDG_CACHE_HOME`) and the case's variables, sources the snippet, enters the
working directory, and clears the screen, then shows the recording once
superline has drawn something. Paths are typed with forward slashes, which
PowerShell, nushell, and Git Bash all accept on Windows. The home is exported
inside the shell rather than on the VHS process because Chrome on Windows
resolves its own app-data folders through `%USERPROFILE%` and exits when that
points at the fixture. A `Wait` that never matches fails the capture after 30
seconds instead of producing a misleading image.

## CI artifacts

The `Terminal snapshots` workflow captures every supported shell on macOS, and
PowerShell and Git Bash through ConPTY on Windows. It builds the pinned VHS, installs
pinned, checksum-verified ttyd, ffmpeg (Windows), and Nerd Font builds, and
uploads `terminal-snapshots-macos` and `terminal-snapshots-windows` artifacts
on success or failure. They are retained for 14 days on every pull request,
`main` push, and manual run.

The job fails when a case's waits or checks fail. The screenshots
deliberately remain build artifacts instead of committed goldens: shell
versions and font rendering vary between runner images, so the PNGs are for
human visual review, while the text checks pin down layout and the normal Rust
tests cover prompt content and escape-style contracts.

## What this does and does not prove

The rig exercises shell startup snippets, each shell's prompt hook, terminal
width calculation, ANSI color, Nerd Font glyphs, multi-row layout, right
prompts, line continuation, exit-status propagation, and that a new prompt
does not clobber the previous one. The `no-newline` test pins down a known
bash limitation: bash has no way to start the prompt on a fresh line after
output without a trailing newline, so the first row shares that line.
Everything is rendered and answered by a pinned xterm.js-based terminal, so it
does not guarantee identical behavior in every native terminal application.
Host-specific settings and timing-sensitive input such as rapid Ctrl-C still
need a focused manual run in that terminal.

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
