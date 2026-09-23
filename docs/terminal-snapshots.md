# Terminal screenshot tests

The terminal snapshot rig launches the branch-local `superline` binary inside
real interactive shells and photographs the prompt with
[VHS](https://github.com/charmbracelet/vhs). VHS runs each shell through
[ttyd](https://github.com/tsl0922/ttyd) on the native pseudoterminal (PTY on
Unix, ConPTY on Windows), renders it with xterm.js in headless Chromium, and
writes PNG screenshots. No desktop session or visible terminal window is
required, and no terminal emulation lives in this repository.

Every capture is a *case*: a superline config, a terminal size (or a list
of widths), a working directory, and a list of steps that type commands, wait
for the screen to match, and take snapshots. The built-in cases live in
[`examples/terminal-snapshot/cases.json`](../examples/terminal-snapshot/cases.json);
you can also pass your own manifest, or build a one-off case on the command
line. Each case runs once per shell and width.

Each snapshot produces a PNG and the matching screen text, which the case can
check with regexes and column-width assertions. Every capture uses a pinned
font, theme, and locale, an isolated home directory, and a deterministic
config, so captures from different platforms are directly comparable. The
grid is rendered at a 36px font, so the resulting PNGs are 2x density and
display crisply on high-DPI screens.

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
  instead of Git Bash, and makes the screen text VHS reads for `Wait+Screen`
  and its text output start at the visible viewport rather than the top of
  the scrollback. The earlier VHS v0.11.0 hangs on Windows with current
  Chrome. Once releases carry the fixes, drop the patch and use the release
  binary directly.

## Run locally

```bash
cargo build --bin superline --example terminal-snapshot
cargo run --example terminal-snapshot -- --vhs target/vhs-bin/vhs
```

That runs every built-in case in every shell on `PATH`. The rig always uses
the `superline` binary from the same target directory, so there is no need to
`cargo install` it. Missing shells are reported and skipped; add
`--require-all` when a missing shell should fail the run.
`SUPERLINE_E2E_VHS` can be used instead of `--vhs`, and without either the rig
looks for `vhs` on `PATH`. Captures run two at a time; change that with
`--jobs`.

Narrow the run with repeatable flags, and list the cases with `--list`:

```bash
cargo run --example terminal-snapshot -- \
  --shell zsh --shell pwsh \
  --case multiline --case widths \
  --columns 50 --columns 200 \
  --vhs target/vhs-bin/vhs
```

`--columns` and `--rows` override the size of every selected case, `--workdir`
runs them in an existing directory, and `--env KEY=VALUE` exports a variable
before the first prompt. `--print` writes each snapshot's screen text to the
terminal as well.

### Output

Everything lands under `--output` (default `target/terminal-snapshots`), one
directory per case:

- `<case>/<platform>-<shell>-<cols>x<rows>-<snapshot>.png` and `.txt`: the
  screenshot and the visible screen text at the same moment.
- `<case>/<platform>-<shell>-<cols>x<rows>.tape`, `.log`, and `.frames.txt`:
  the generated VHS tape, VHS's output, and every screen dump VHS recorded,
  for diagnosing a failed capture.
- `<platform>-summary.json`: every run with its status (`pass`, `fail`,
  `xfail`, or `xpass`), snapshot paths, screen text, and failed checks.

The run exits non-zero if any run fails or unexpectedly passes.

### One-off captures

Passing `--config` or any step flag builds a single case from the command line
instead of reading the manifest. Steps run in the order given:

```bash
cargo run --example terminal-snapshot -- \
  --shell zsh \
  --config ~/.config/superline/config.json \
  --workdir "$PWD" \
  --columns 80 --columns 140 --rows 10 \
  --run 'git status --short' \
  --exit 3 \
  --output target/prompt-debug \
  --vhs target/vhs-bin/vhs
```

The step flags are `--run <CMD>` (type and press Enter), `--exit <N>` (a
command that exits with status N in the current shell), `--type <TEXT>`,
`--key <KEY>` (a VHS key such as `Ctrl+C` or `'Tab 2'`), `--wait <REGEX>`,
`--sleep <DURATION>`, and `--snapshot <NAME>`. Without any `--snapshot`, one is
taken after the first prompt (`0`) and after each `--run` or `--exit` (`1`,
`2`, ...), once the typed command is on screen. Screen text is printed for
one-off runs. `--name` sets the case name used for the output directory
(default `adhoc`).

A theme file the config names relative to itself is copied along with it.
Widgets with background lookups (for example `ai_usage`) start cold in the
isolated home; remove them from a copy of the config if their probes are
unwanted.

## Writing cases

A manifest is a JSON array of cases. Only `name` and `steps` are required:

```json
{
  "name": "widths",
  "description": "Right sides stay flush with the edge",
  "config": { "rows": [{ "left": ["shell", "cmd"], "right": [{ "text": "edge" }, { "padding": 0 }] }] },
  "columns": [40, 80, 160],
  "rows": 6,
  "steps": [
    { "wait": "{{shell}}{{sep}}{{ok}}" },
    {
      "snapshot": {
        "name": "prompt",
        "widths": [{ "line": "edge", "min": "{{columns}} - 1", "max": "{{columns}}", "shells": ["fish", "zsh", "nu"] }]
      }
    }
  ]
}
```

| Field | Meaning |
| --- | --- |
| `name` | Output directory and `--case` name (letters, digits, `-`, `_`). |
| `description` | Shown by `--list`. |
| `config` | An inline superline config, or a path relative to the manifest. Defaults to `examples/terminal-snapshot/config.json`. |
| `columns` | A width or a list of widths; one run each. Default 100. |
| `rows` | Terminal height. Default 12. |
| `dir` | Working directory, relative to the scratch root, created if missing. Default `superline-e2e`. |
| `env` | Variables exported before the first prompt. |
| `shells`, `platforms` | Only run on these shells, or on these platforms (`macos`, `linux`, `windows`). |
| `xfail` | A known bug: a reason string, or `{ "reason", "shells", "platforms" }` to scope it. A failing run is reported as `XFAIL` and does not fail the suite; a passing run is reported as `XPASS` and does, so the marker is removed once the bug is fixed. |
| `steps` | What to do, in order. |

Each step is an object with one key:

| Step | Meaning |
| --- | --- |
| `{ "run": CMD }` | Type a command and press Enter. |
| `{ "type": CMD }` | Type without pressing Enter. |
| `{ "key": "Ctrl+C" }` | A VHS key command, optionally with a repeat count (`"Tab 2"`). |
| `{ "wait": REGEX }` | Wait (up to 30s) until the visible screen matches. |
| `{ "wait_line": REGEX }` | Wait until the cursor line matches. |
| `{ "sleep": "500ms" }` | Pause. |
| `{ "snapshot": NAME }` | Take a PNG and the screen text. |
| `{ "snapshot": { "name", "expect", "reject", "widths" } }` | Take a snapshot and check its text. |

`CMD` is a string, `{ "exit": N }` for a command that exits with status N in
the current shell, or an object mapping shell names to commands with a
`default` for the rest.

Snapshot checks run on the screen text, with lines joined by `\n`:

- `expect` and `reject` list regexes the text must or must not match. An entry
  can be `{ "regex": ..., "shells": [...] }` to apply to some shells only.
- `widths` checks the terminal width of every line matching `line`: `width`
  for an exact value, or `min` and `max`. Bounds are numbers or simple sums
  such as `"{{columns}} - 1"`, and `shells` limits the check. Double-width
  characters count as two columns.

Regexes use RE2 syntax in `wait` steps and Rust `regex` syntax in snapshot
checks; the common subset (`\n`, `\A`, `\z`, `(?s:...)`, `\x{E0B0}`) works in
both. Step text and regexes can use these placeholders:

| Placeholder | Value |
| --- | --- |
| `{{shell}}`, `{{columns}}`, `{{rows}}` | The current run's shell name and size. |
| `{{last}}` | The last typed text, escaped as a regex. |
| `{{sep}}`, `{{round}}` | The chevron (`\x{E0B0}`) and round (`\x{E0B4}`) separators, as regex escapes. |
| `{{ok}}` | The `cmd` widget's success mark (`\x{F105}`), as a regex escape. |

Every case should start by waiting for its first prompt, and wait for the
result of each command before a snapshot: the rig only pauses for a second
before capturing. Right sides on the last row are drawn by fish, zsh, and
nushell but not bash or PowerShell, and zsh leaves one column free to the
right of its right prompt, so width checks on the last row need a `shells`
list and a one-column range.

## How a capture works

`examples/terminal-snapshot.rs` is a thin orchestrator. For each case, shell,
and width it:

1. creates a scratch home with the case's config and working directory;
2. runs `superline init <shell>` and saves the snippet as a startup file
   (PowerShell's copy also gets an explicit `--config` path);
3. turns the steps into VHS commands, fills
   `examples/terminal-snapshot/tape.template`, and writes the tape next to the
   screenshots;
4. runs VHS with a pinned locale and `PATH` pointed at the branch-local
   binary;
5. matches each `Screenshot` to the screen dump VHS recorded after it, saves
   that as the snapshot's text, and runs the snapshot's checks.

The tape hides the setup, waits for the shell's stock prompt, types one line
that exports the isolated home (`HOME`, `USERPROFILE`, `XDG_CONFIG_HOME`,
`XDG_CACHE_HOME`) and the case's variables, sources the snippet, enters the
working directory, and clears the screen, then shows the recording once
superline has drawn something. Paths are typed with forward slashes, which
PowerShell, nushell, and Git Bash all accept on Windows. The home is exported
inside the shell rather than on the VHS process because Chrome on Windows
resolves its own app-data folders through `%USERPROFILE%` and exits when that
points at the fixture. A `wait` that never matches fails the capture after
30 seconds instead of producing a misleading image.

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
does not clobber the previous one. The built-in `wide-chars` and `no-newline`
cases are marked `xfail`: superline measures segment widths in characters
rather than terminal cells, and bash has no way to start the prompt on a fresh
line after output without a trailing newline. Everything is rendered and
answered by a pinned xterm.js-based terminal, so it does not guarantee
identical behavior in every native terminal application. Host-specific
settings and timing-sensitive input such as rapid Ctrl-C still need a focused
manual run in that terminal.

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
