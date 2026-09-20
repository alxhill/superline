# superline

[![crates.io](https://img.shields.io/crates/v/superline.svg)](https://crates.io/crates/superline)

A fast, opinionated powerline-style prompt written in Rust. It understands git and GitHub, detects Rust, Python,
Node and Java project environments, and can show your Claude or Codex subscription usage, with async rendering
support for the slower lookups.

![Shell with pyenv showing](https://raw.githubusercontent.com/alxhill/superline/main/with_pyenv.png)

With the [`gh`](https://cli.github.com) CLI installed, it also links to the current branch's pull request and shows
its CI status:

![Shell with PR link and status check](https://raw.githubusercontent.com/alxhill/superline/main/with_status.png)

superline started as a fork of [cirho/powerline-rust](https://github.com/cirho/powerline-rust), itself a pure-Rust
take on [powerline-shell](https://github.com/b-ryan/powerline-shell), and has since grown a set of opinionated but
configurable modules and themes.

## Highlights

- **Fast**: a few tens of milliseconds per prompt, git status included.
- **Lazy**: backends only run when needed, so there is no git cost outside a git repo and no Python cost outside a
  project.
- **Never blocks**: slow lookups (git status on big repos, PR status, AI usage) are refreshed in the background and
  served from a cache.
- **Flexible layout**: multiple rows, each with an optional right-aligned side.
- **Themeable**: two built-in themes, or point at your own theme JSON file.
- **Any shell**: fish, zsh, bash, PowerShell and nushell are all supported by `superline install`.

## Installation

### 1. Install a Nerd Font

superline relies on [Nerd Font](https://www.nerdfonts.com/) glyphs. Configure your terminal to use one, otherwise
most segments will not render correctly. Meslo LG S is recommended and can be downloaded in patched form
[here](https://github.com/ryanoasis/nerd-fonts/releases/download/v3.2.1/Meslo.zip).

If you run into alignment issues in iTerm2, try enabling "Use built-in Powerline glyphs" in the profile's text
settings, even when using a Nerd Font:

![iTerm2 Profile configuration](https://raw.githubusercontent.com/alxhill/superline/main/iterm_config.png)

### 2. Install the binary

With Homebrew:

```bash
brew install alxhill/superline/superline
```

With [cargo-binstall](https://github.com/cargo-bins/cargo-binstall), which downloads a prebuilt binary and needs no
Rust toolchain:

```bash
cargo binstall superline
```

Prebuilt binaries are published for macOS (Apple Silicon), Linux (x86-64 and arm64) and Windows (x86-64).

Or build from source via crates.io (cargo's bin directory must be on your `$PATH`):

```bash
cargo install superline
```

### 3. Hook it into your shell

```bash
superline install <shell>
```

Then reload your shell config. Supported shells are `fish`, `zsh`, `bash`, `pwsh` (PowerShell) and `nu` (nushell).
The command appends a loader to the shell's default config file and is safe to run more than once.

- **PowerShell** - appends to `$PROFILE`, creating it if needed. Windows PowerShell works but sees far less testing
  than the Unix shells and Git Bash, so expect rougher edges; [`docs/powershell-testing.md`](docs/powershell-testing.md)
  lists the known caveats.
- **nushell** - appends a loader to `config.nu` (found via `$nu.config-path`) that regenerates the prompt script into
  nushell's vendor autoload directory on startup. Requires nushell 0.96 or newer.

If you'd rather manage the loader yourself, `superline init <shell>` prints the snippet without touching any files.

## Configuration

On first run superline writes a default config to `$HOME/.config/superline/config.json`. Edits take effect on the
next prompt - no reload needed. [`example_config.json`](example_config.json) shows a complete setup and
`src/config.rs` is the authoritative definition of every option.

A config has a `theme` and a list of `rows`:

```json
{
  "theme": "rainbow",
  "rows": [
    {
      "left": [
        "read_only",
        { "cwd": { "max_length": 60, "wanted_seg_num": 5 } },
        "git",
        "pr"
      ],
      "right": [
        "python",
        "cargo"
      ]
    },
    {
      "left": [
        "shell",
        "cmd"
      ]
    }
  ]
}
```

Each row has a required `left` array and an optional `right` array of segments. superline prints every row but the
last in full, left and right. The last row's `right` is drawn by the shell's own right-prompt mechanism
(`fish_right_prompt`, `RPS1` in zsh, `PROMPT_COMMAND_RIGHT` in nushell), so it stays put as you type. Bash and
PowerShell have no right prompt, so on those shells the last row's `right` is not shown.

Every module can be written either as a bare string or as an object with options, so `"git"` and `{ "git": {} }`
are equivalent. Modules with required options (`cwd`, `last_cmd_duration`, `ai_usage`, `padding`, `separator`) must
use the object form.

### Layout

Three special segments control how modules are grouped and joined. They can appear anywhere in a `left` or `right`
array.

#### separator

Sets the shape used between segments. Options are `"chevron"` (the default), `"round"` and `"angle_line"`. It is
stateful: the style applies to every following segment on the same side until changed again.

```json
{ "separator": "round" }
```

#### small_spacer / large_spacer

Insert a blank segment with a black background as part of the current block.

```json
"small_spacer"
```

#### padding

Ends the current block of segments and clears the background. The next module starts a new block with a reversed
separator. The number is the gap width in cells; `0` is common at the end of a `right` array.

```json
{ "padding": 2 }
```

### Modules

#### cwd

The current working directory, shortened to `wanted_seg_num` path components and at most `max_length` characters.
Both are required. Set `resolve_symlinks` to `true` to show the real path instead of the one you `cd`'d into.

```json
{ "cwd": { "max_length": 60, "wanted_seg_num": 5, "resolve_symlinks": false } }
```

#### read_only

Shows a lock icon when the current directory is not writable.

```json
"read_only"
```

#### cmd

The prompt character shown before your input. It turns red and shows the exit code when the previous command failed.

```json
"cmd"
```

#### last_cmd_duration

How long the previous command took, shown only when it ran for at least `min_run_time` milliseconds. The option is
required.

```json
{ "last_cmd_duration": { "min_run_time": 50 } }
```

#### shell

The name of the running shell (`fish`, `zsh`, ...).

```json
"shell"
```

#### hostname and user

The hostname and the current username.

```json
"hostname"
```

`"host"` remains accepted as a compatibility alias.

#### jobs

Shows background jobs owned by the current shell, including stopped jobs. It is
hidden with no jobs, shows `✦` for one job, and shows `✦N` for two or more jobs.

```json
"jobs"
```

#### battery

Shows a low-battery warning with the current charge percentage and charging
state. It appears when the aggregate charge is 10% or lower and stays hidden
when no battery is available or the charge is above that threshold.

```json
"battery"
```

#### kubernetes

The active Kubernetes context and, when it is set in that context, its namespace. The module reads
`$KUBECONFIG` (a platform-separated list of kubeconfig files) or `$HOME/.kube/config` and refreshes the
lookup in the background so a large kubeconfig never blocks prompt rendering. It is hidden when there is no
readable kubeconfig or no current context.

```json
"kubernetes"
```

The default label is `☸ context` or `☸ context (namespace)`. Set `modules.kubernetes.icon` in a custom theme to
change the marker or set it to an empty string to hide it.

#### time

The current time. `format` is a [strftime](https://docs.rs/chrono/latest/chrono/format/strftime/index.html)
string and defaults to `%H:%M:%S`.

```json
{ "time": { "format": "%H:%M" } }
```

#### git

The current branch and working-tree status: modified, staged and untracked counts, plus ahead/behind counts against
the upstream. A GitHub logo appears whenever the repo has a remote, and links to the repository's web page (derived
from the `origin` fetch URL); the ahead/behind counts beside it need an
upstream tracking ref that still resolves, so they are absent on a branch that was never pushed or whose remote
branch has been deleted.

A detached HEAD shows the short commit hash. When that commit is the tip of a branch (a worktree created with
`git worktree add --detach`, or `git checkout origin/main`) the branch follows it, as `1a2b3c4 -> main`; local
branches take precedence over remote-tracking ones, and `main`/`master` over other names.

Status collection waits up to `status_timeout_ms` (250 by default). If it takes longer, the last cached result is
shown while a refresh continues in the background for the next prompt. Before anything is cached the segment shows
`loading…`.

```json
{ "git": { "status_timeout_ms": 250, "backend": "auto" } }
```

Status is produced by one of two backends, chosen with `backend`:

- `cli` shells out to the `git` binary. It is the fastest option on large working trees because it is the only
  backend that uses git's own [untracked cache](https://git-scm.com/docs/git-update-index#_untracked_cache) and
  fsmonitor; enabling `core.untrackedCache` in a big repo typically cuts status time by two thirds. It needs `git` on
  your `PATH`.
- `gitoxide` walks the working tree in-process with pure Rust. It needs no external binary, and is faster on small
  repos where the CLI's process-spawn overhead dominates.
- `auto` (the default) picks between them from the size of `.git/index`: the CLI for large working trees, gitoxide
  for small ones. It falls back to gitoxide whenever `git` isn't on `PATH`.

#### pr

A clickable link to the GitHub pull request for the current branch, looked up via the `gh` CLI. The segment colour
reflects the PR state (draft, open, merged, closed). With `status` on (the default) a coloured dot follows the PR
number showing CI check status: green for success, red for failure, yellow for pending.

The lookup runs in the background and is cached, so it never blocks the prompt - the link appears on a later prompt
once the result is ready. The module is skipped entirely on `main`, `master` and `develop`.

```json
{ "pr": { "status": false } }
```

#### ai_usage

Claude or Codex subscription usage, read via the provider's CLI on `PATH`. superline refreshes it in the background
and caches the result, so rendering never waits on a provider request. Add the module more than once to show both
providers, or the same provider with different windows and styles. Provider labels use the Nerd Font
[`cod-openai`](https://www.nerdfonts.com/cheat-sheet?q=cod-openai) (`U+EC81`) and
[`cod-claude`](https://www.nerdfonts.com/cheat-sheet?q=cod-claude) (`U+EC82`) glyphs.

![Claude and Codex usage widgets using the sparkline display](https://raw.githubusercontent.com/alxhill/superline/main/ai_usage.png)

`provider` is required and is `"claude"` or `"codex"`. Everything else is optional.

**Windows.** `session` and `weekly` (both default `true`) toggle the five-hour and seven-day rate-limit windows.
`fable` adds the Claude-only weekly Fable window and is ignored for Codex. Labels default to `"5h "`, `" 7d "` and
`" F "`, with the spaces keeping adjacent windows apart; override them with `session_label`, `weekly_label` and
`fable_label`, or set one to `""` to drop a label.

```json
{ "ai_usage": { "provider": "claude", "fable": true, "session_label": "" } }
```

**Display styles.** `display` picks how each window is drawn. The examples show the five-hour window at 61% used with its default label.

| Style | Aliases | Example | Rendering |
|-------|---------|---------|-----------|
| `"percentage"` (default) | `percent`, `percents`, `percentages`, `pct` | `5h 61%` | Percent used as a number. |
| `"bar"` | `bars` | `5h ▄▄▄▁▁` | A five-cell half-height bar that fills left to right. |
| `"capped_bar"` | `capped_bars`, `capped` | `5h ▗▄▄▄▁▁▖` | The same bar with end caps. |
| `"block"` | `blocks` | `5h ███░░` | Five full-height cells, shaded when empty. |
| `"sparkline"` | `sparklines`, `spark`, `sparks` | `5h ▅` | One glyph per window. |
| `"numeric"` | `number`, `numbers`, `num` | `5h 61%` | Raw figures for the credits lane; same as `percentage` for the rate-limit windows. |

```json
{ "ai_usage": { "provider": "codex", "display": "bar" } }
```

**Threshold.** `threshold` is a percent-used warning level. When any visible lane crosses it, the whole widget
background switches to the theme's `modules.ai_usage.threshold_bg` colour.

```json
{ "ai_usage": { "provider": "claude", "display": "sparkline", "threshold": 80 } }
```

**Credits.** `credits` adds a lane for usage credits: Claude reports dollars spent against the credit limit and Codex
reports the per-seat budget as a plain count. `credits_display` accepts the same styles as `display` plus
`"numeric"` for raw figures such as `$50/$100` or `411/12000`, and follows `display` when unset. `credits_label`
defaults to `" C "`. Set `credits_only_when_limited` to show the lane only once a session or weekly window has hit
100%, which is when the provider starts drawing on credits. A visible credits lane counts towards `threshold`.

```json
{ "ai_usage": { "provider": "claude", "credits": true, "credits_display": "numeric", "credits_only_when_limited": true } }
```

**Session countdown.** `session_time_remaining` shows how long until the session window resets. To show it only once
the session is nearly full, set `session_time_remaining_only_at_limit` to a fraction from `0` to `1`; `0.8` shows it
at 80% and above.

```json
{ "ai_usage": { "provider": "claude", "session_time_remaining": true, "session_time_remaining_only_at_limit": 0.8 } }
```

**States.** Until the first reading is cached the widget shows `…`. If the provider CLI isn't on `PATH` it shows `?`.
If the CLI is installed but not logged in it shows a logged-out user icon (``) until you log in.

### Language modules

`python`, `node`, `java` and `cargo` share one behaviour and differ only in how they detect a project and
which files can pin a version. Each shows its language icon when the current directory belongs to a project, and
adds the version when one is pinned. Every one takes a `version` option; it defaults to `true`, and setting it to
`false` leaves just the icon. `python` and `node` were previously called `python_env` and `nvm`; the old names
still work in both the config and theme files.

```json
{ "node": { "version": false } }
```

Versions come first from [mise](https://mise.jdx.dev) configs (`mise.toml`, `.mise.toml`,
`.config/mise/config.toml`, `.tool-versions` and their `.local` variants), searched from the current directory
upwards with the nearest declaration winning. A mise version beats one from a language-specific file such as
`.sdkmanrc`, since mise is what actually puts the tool on the path. The global mise config in `$HOME` is
deliberately ignored: these modules report what a project pins, and a global `python` entry would otherwise light
them up in every directory.

A `󱁤` marker (the Nerd Font
[`md-tools`](https://www.nerdfonts.com/cheat-sheet?q=md-tools) glyph, `U+F1064`) follows any version that came
from mise. Themes can change or hide it with each module's `mise_icon`
property. The marker stays even when `version` is `false`, since it says who manages the tool rather than which
one is pinned.

#### python

- **Detects** an active virtual env (venv, conda or mamba), or a directory pinned by `.python-version` or containing
  a `pyproject.toml`.
- **Shows** the virtual env name when one is active; `venv: false` hides it.
- **`version`** defaults to `true`. Inside a venv it reports the interpreter the env was built from, read from the
  env's `pyvenv.cfg` (venv, uv, virtualenv) or `conda-meta`. An env with neither is asked directly: the interpreter
  runs in the background and its answer is cached, with `…` shown until it lands. Outside a venv it reports the
  pinned version.

```json
{ "python": { "version": true, "venv": false } }
```

#### node

- **Detects** the Node version nvm has activated, or one pinned by `.nvmrc`.

#### java

- **Detects** a version pinned by `.sdkmanrc`.
- **Shows** the JDK distribution (corretto, Temurin, ...) as well as the major version; `jdk: false` hides it, and
  with `version` also off only the icon remains.
- Also accepted under its former name, `sdkman`.

```json
{ "java": { "jdk": false } }
```

#### cargo

- **Detects** a `Cargo.toml` in the current directory.
- **Pins** via `rust-toolchain.toml` or the legacy `rust-toolchain`, searched upwards from the current directory as
  rustup does, so workspace members pick up the pin at the workspace root.

### Themes

`theme` is `"rainbow"`, `"simple"`, or a path to a theme JSON file. Paths starting with `/` are absolute; anything
else is resolved relative to the config directory (`$HOME/.config/superline/`). If a custom theme fails to load,
superline falls back to `rainbow`.

A theme file has two keys, `defaults` and `modules`:

```json
{
  "defaults": { "fg": "green", "bg": "black" },
  "modules": {
    "cargo": { "fg": "black", "bg": "burnt_orange" },
    "cwd": { "path_fg": "white", "bg_colors": ["red", "orange", "yellow", "green"] },
    "readonly": { "fg": 254, "bg": 124 }
  }
}
```

- **defaults** - the `fg` and `bg` used for anything a module doesn't set.
- **modules** - per-module overrides. Most modules accept `fg` and `bg`; some have extra colours (`git` has
  `staged_bg`, `pr` has `open_bg`, `cwd` takes a `bg_colors` array) or strings (`cmd.user_symbol`, `pr.icon`,
  `kubernetes.icon`, and `mise_icon` on the language modules - set a marker to `""` to hide it). Anything omitted falls back to
  `defaults`.

Note that the `read_only` module is themed as `readonly`. The `node` and `python` modules also still accept their
old theme keys, `nvm` and `py`.

Colours are a name from `src/colors.rs` (for example `"green"` or `"warning_red"`) or an ANSI 256-colour code from
`0` to `255`. [`example_theme.json`](example_theme.json) covers every module, and `src/themes/custom.rs` lists every
module name and property.

## Commands

| Command | What it does |
|---------|--------------|
| `superline install <shell>` | Append the prompt loader to the shell's config file. |
| `superline init <shell>` | Print the loader snippet to stdout instead. |
| `superline config` | Open the config file in `$EDITOR`. |
| `superline clear-caches` | Wipe cached git status, PR lookups and AI usage so the next prompt starts cold. |

## Debugging a slow prompt

Set `SUPERLINE_DEBUG=1` to have superline print a timing report to **stderr** after it renders. The prompt on stdout is
unchanged, so this works in a live shell as well as from a one-off `superline show`.

```console
$ SUPERLINE_DEBUG=1 superline show -s 0 -c 200 fish >/dev/null
superline debug
  startup                 0.1ms
  prune caches            0.1ms
  config                  0.7ms
  theme                   0.6ms
  render                141.7ms
    row 1               140.9ms
      Cwd                 0.0ms
      Git               133.9ms
        git             133.8ms  refresh finished during wait
      Pr                  6.8ms
      Usage               0.1ms
        usage             0.1ms  hit fresh (age 14s)
    row 2                 0.3ms
      Node                0.1ms
    print                 0.0ms
  total                 143.1ms
```

Each module is timed where the prompt draws it, and any cached lookup it made is listed underneath with how it was
served: `hit fresh`, `hit stale (…), refresh spawned`, `miss, refresh spawned`, `refresh finished during wait`, or
`wait timed out, serving cache (…)`. A module with no cache line under it did all of its work inline. Rows render one
after another, so the times down the tree add up to the `total` wall clock.

## Using superline as a library

For the fastest possible prompt you can skip the config file entirely and compile your layout into a small Rust
program. `examples/minimalistic.rs` and `examples/rainbow.rs` are complete, runnable starting points.

```rust
use superline::modules::*;
use superline::powerline::{PowerlineRightBuilder, PowerlineShellBuilder};
use superline::terminal::Shell;
use superline::themes::SimpleTheme;

fn main() {
    superline::Powerline::builder()
        .set_shell(Shell::Bare)
        .add_module(Cwd::<SimpleTheme>::new(45, 4, false))
        .add_module(Git::<SimpleTheme>::new())
        .add_module(ReadOnly::<SimpleTheme>::new())
        .add_module(Cmd::<SimpleTheme>::new("0"))
        .render(0);
}
```

Themes are types that implement each module's `*Scheme` trait, so a custom theme is just a struct with a few impls:

```rust
use superline::modules::*;
use superline::themes::DefaultColors;
use superline::Color;

struct Theme;

impl DefaultColors for Theme {
    fn default_fg() -> Color { Color(15) }
    fn default_bg() -> Color { Color(236) }
}

impl CmdScheme for Theme {
    fn cmd_failed_bg() -> Color { Color(161) }
}

fn main() {
    let mut prompt = superline::Powerline::new();
    prompt.add_module(Cmd::<Theme>::new("0"));
    // ...
}
```
