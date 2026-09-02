# superline

[![crates.io](https://img.shields.io/crates/v/superline.svg)](https://crates.io/crates/superline)

_Forked from [cirho/powerline-rust](https://github.com/cirho/powerline-rust) and adjusted for personal taste_

superline supports git and github natively, and detects rust, python, node and java environments - from
`.sdkmanrc`, `.nvmrc` and `.python-version`, or from a [mise](https://mise.jdx.dev) config.

![Shell with pyenv showing](https://raw.githubusercontent.com/alxhill/superline/main/with_pyenv.png)

It integrates with the `gh` shell command to provide PR and CI status check display as well.

![Shell with PR link and status check](https://raw.githubusercontent.com/alxhill/superline/main/with_status.png)

superline is a pure-rust version of [powerline-SHELL](https://github.com/b-ryan/powerline-shell). It's heavily
inspired
by it, but focuses on minimalizing time of execution and supporting a limited subset of features.

## Advantages

- blazing fast (~15ms when reading from a config file, 9ms for a compiled binary)
- runs backends only when needed (huge time improvements when not in a git repo or python venv)
- optional caching git results in memory or file
- supports fully compiled prompts (see `examples/rainbow.rs`) or can read from a provided config file.
- new themes and modules can be added easily (currently only Rainbow and Simple are included)
- supports multiline prompts as well as showing info on the right hand side of the terminal.

## Installation

superline relies on [Nerd Font](https://www.nerdfonts.com/) unicode characters - configure your terminal to use a
Nerd Font, otherwise most segments will not render correctly. Meslo LG S is recommended and can be
downloaded in patched form [here](https://github.com/ryanoasis/nerd-fonts/releases/download/v3.2.1/Meslo.zip).

iTerm2 users are recommended to enable the "Use builtin Powerline glyphs" option even when using a Nerd Font as this
seems to fix some character alignment issues.

![iTerm2 Profile configuration](https://raw.githubusercontent.com/alxhill/superline/main/iterm_config.png)

To install the package, just run the following:

```bash
cargo install superline
superline install <shell name>
```

Then reload your shell's config. Superline will modify the default config file for the shell you choose - currently,
`fish`, `zsh`, `bash`, and `pwsh` (PowerShell). For example, `superline install pwsh` appends the loader to your
PowerShell profile (`$PROFILE`), creating it if necessary. PowerShell compiles for Windows (the Unix-only bits live
behind `src/platform.rs`) but isn't yet runtime-tested there — see
[`docs/powershell-testing.md`](docs/powershell-testing.md) for the cross-platform testing plan and Windows caveats.

Cargo's bin directory must be in your `$PATH` for the `superline` command to be available.

### Git backends

The `git` segment can be powered by one of three interchangeable backends, selected at compile time via cargo
features. They produce identical output but have different performance and build-dependency trade-offs:

- **`gitoxide`** (default) — uses the pure-Rust [`gix`](https://crates.io/crates/gix) crate, with no C dependencies.
- **`libgit`** — uses the `git2` bindings to `libgit2`.
- **the `git` CLI** — the fallback when no backend feature is enabled; shells out to the `git` binary on `$PATH`.

When more than one backend feature is enabled the precedence is `gitoxide` > `libgit` > the CLI fallback. To build
against a specific backend:

```bash
cargo install superline                                        # gitoxide (default)
cargo install superline --no-default-features --features libgit # libgit2
cargo install superline --no-default-features                   # git CLI fallback
```

## Customization

Superline will create a default config file at `$HOME/.config/superline/config.json`. You can edit it to make
changes, which will be reflected immediately.

### Config file

`config.rs` has the full definition of all valid types in the config directory, `example_config.json` shows a complete
configuration setup.

Two themes are built in, "rainbow" and "simple" (the latter is not recommended). You can also point `theme` at a
custom theme JSON file - see [Themes](#themes) below.

The example_config.json shows most of the options available:

```json
{
  "theme": "rainbow",
  "rows": [
    {
      "left": [
        "small_spacer",
        {
          "cwd": {
            "max_length": 60,
            "wanted_seg_num": 4,
            "resolve_symlinks": false
          }
        },
        "read_only",
        "small_spacer",
        {
          "git": {
            "status_timeout_ms": 250
          }
        },
        { "pr": { "status": true } }
      ],
      "right": [
        {
          "separator": "round"
        },
        "python_env",
        {
          "padding": 0
        }
      ]
    },
    {
      "left": [
        {
          "last_cmd_duration": {
            "min_run_time": "0ms"
          }
        },
        "cmd"
      ]
    }
  ]
}
```

You can add as many rows as desired. Each row has `left` and `right` properties for adding new segments - `left` is
required, while `right` is optional. The final row should have only a `left` property so the cursor can show next to
it - it's not currently possible to have a value showing on the right side next to a one-line prompt.

Inside the `left` and `right` arrays, you can add the following sections to for showing content:

* **cmd** - show `>` before user input. Turns red and shows the error code if the previous command fails.
* **cwd** - show the current working directory, with configurable size and max segments.
* **cmd_duration** - show the time taken by the last command if it takes longer than `min_run_time`
* **host** - the hostname
* **user** - the current user
* **read_only** - show a lockfile icon if the current directory is read only
* **time** - show the current time, with an optional "format" - this has to be present, but can be null
* **usage** - show Claude or Codex subscription usage via the corresponding provider CLI on `PATH`. Superline
  refreshes it in the background and caches the result, so prompt rendering never waits for a provider request. Set
  `provider` to `"claude"` or `"codex"`, choose the five-hour and weekly lanes with `session` / `weekly` (both
  default to `true`), and set
  `display` to `"percentage"` (the default), `"bar"` (a five-cell shaded bar), or `"sparkline"` (one glyph per
  window). The values are percent used. Set `threshold` to a percent-used warning level; configure the warning
  background in the theme as `modules.usage.threshold_bg`. It replaces the entire widget background whenever either
  enabled window crosses the threshold. For example:
  `{ "usage": { "provider": "codex", "session": true, "weekly": true, "display": "bar", "threshold": 80 } }`.
  Add the module more than once to show both providers or different windows/display styles. Provider labels use the
  Nerd Font OpenAI (`U+EC81`) and Claude (`U+EC82`) glyphs.
* **python_env** - if a virtual env (venv, conda, mamba) is active, show the name and current version of python.
  Otherwise, show the version pinned by a mise config or `.python-version`, or a bare python icon in any directory
  containing a `pyproject.toml`
* **nvm** - show the active node version, falling back to the version pinned by a mise config or `.nvmrc`
* **java** - show the java version and distribution pinned by a mise config or `.sdkmanrc`. Also accepted under its
  former name, `sdkman`
* **cargo** - show a crab icon if a `Cargo.toml` file is present in the current dir, plus the toolchain version if a
  mise config pins one
* **git** - show the current git branch and status of the repo (modified, staged, and untracked files, plus git remote
  ahead/behind stats). The GitHub logo appears whenever the repo has a remote configured; the ahead/behind counts
  beside it need an upstream tracking ref that still resolves, so they're absent on a branch that was never pushed or
  whose remote branch has since been deleted. Status collection waits up to `status_timeout_ms` milliseconds (250 by default); if it takes
  longer, the most recent cached output is shown while the refresh continues in the background for the next prompt.
  Before the first result is cached, the segment displays `loading…` instead. The string shorthand `"git"` uses the
  default timeout.
* **pr** - show a clickable link to the GitHub PR for the current branch (via the [`gh`](https://cli.github.com)
  CLI), if one exists. The segment colour reflects the PR state (draft, open, merged, closed). When the `status` option
  is enabled (the default), a coloured dot is appended after the PR number reflecting the CI check status - green for
  success, red for failure, yellow for pending. The lookup runs in the background and is cached, so it never blocks the
  prompt - the link appears on a subsequent prompt once the result is ready. Skipped entirely on `develop`, `main`, and
  `master`. Unlike most segments, `pr` is written as an object so its options can be set:
  `{ "pr": { "status": false } }` shows just the PR number with no check dot.

#### mise

The `java`, `nvm`, `python_env` and `cargo` segments read tool versions from [mise](https://mise.jdx.dev) configs
(`mise.toml`, `.mise.toml`, `.config/mise/config.toml`, `.tool-versions`, and the `.local` variants). Configs are
searched from the current directory upwards, with the nearest declaration of a tool winning, and a version from mise
takes precedence over one from a language-specific file such as `.sdkmanrc`, since mise is what actually puts the tool
on the path. A `󱁤` marker is shown next to any version that came from mise; themes can change or hide it
with the `mise_icon` property.

The global config in `$HOME` is deliberately not read - these segments report the tools a project pins, so a global
`python` entry would otherwise light them up in every directory.

There are also three ways to modify the layout:

* **separator** - change the style between segments (see screenshot above). Options are "chevron" and "round". This
  command is stateful, and will apply to all subsequent segments on the same section until overridden. The default is "
  chevron"
* **small_spacer** and **large_spacer** - show a segment as part of the current block with a black background
* **padding** - end the current collection of segments and clear the background. The next segment will start with a
  reversed separator separating it from the previous command.

Usage examples of most of these can be found in the config file shown above.

### Themes

`theme` can be `"rainbow"`, `"simple"`, or a path to a theme JSON file. Paths starting with `/` are absolute;
anything else is resolved relative to the config directory (`$HOME/.config/superline/`). If a custom theme fails to
load, superline falls back to `rainbow`.

A theme file has two keys: `defaults` and `modules`.

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

* **defaults** - the `fg`/`bg` used for any colour a module doesn't set.
* **modules** - per-module overrides, keyed by module name. Most modules accept `fg` and `bg`; some have extra
  properties (e.g. `git` has `staged_bg`, `pr` has `open_bg`, `cwd` takes a `bg_colors` array). A few accept string
  properties such as `cmd.user_symbol`, `pr.icon`, or the `mise_icon` of a language module (set it to `""` to hide
  the mise marker). Any property you omit falls back to `defaults`.

Colours are either a name (defined in `src/colors.rs`, e.g. `"green"`, `"warning_red"`) or an ANSI 256-colour code
(`0`–`255`). See [`example_theme.json`](https://github.com/alxhill/superline/blob/main/example_theme.json) for a full
example covering every module, and `src/themes/custom.rs` for the complete list of module names and properties.

## Custom program

You can also create a separate rust program to fully customize the appearance. This allows creating a new theme too.

```rust
use superline::{modules::*, theme::SimpleTheme};

fn main() {
    let mut prompt = superline::Powerline::new();

    prompt.add_module(User::<SimpleTheme>::new());
    prompt.add_module(Host::<SimpleTheme>::new());
    prompt.add_module(Cwd::<SimpleTheme>::new(45, 4, false));
    prompt.add_module(Git::<SimpleTheme>::new());
    prompt.add_module(ReadOnly::<SimpleTheme>::new());
    prompt.add_module(Cmd::<SimpleTheme>::new());

    println!("{}", prompt);
}


```

### Cache untracked files

Git module can be slower on repos with big number of untracked files. Read about caching untracked
files  [here](https://git-scm.com/docs/git-update-index).

### Custom theme

```rust
use superline::{modules::*, terminal::Color};

struct Theme;

impl CmdScheme for Theme {
    fn cmd_passed_fg() -> Color {
        Color(15)
    }

    fn cmd_passed_bg() -> Color {
        Color(236)
    }

    fn cmd_failed_bg() -> Color {
        Color(161)
    }

    fn cmd_failed_fg() -> Color {
        Color(15)
    }
}


fn main() {
    let mut prompt = superline::Powerline::new();
    prompt.add_module(Cmd::<Theme>::new());

    ...
```

## TODO

- [x] Support NVM enviroments
- [x] Support SDKMAN / Java enviroments
- [x] Switch to cleaner/JSON-first theme structure
- [x] Add a `superline install` command to auto-modify shell config
- [x] Change git icon/name based on branch vs commit vs merging
- [x] Native "right prompt" support on final line (zsh + fish only)
- [ ] Improve display when there aren't enough columns for the whole prompt (e.g truncate paths, show from left not
  right)
