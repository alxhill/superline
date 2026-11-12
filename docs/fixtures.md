# Fixtures

Most widgets show something about the machine or the outside world: the
hostname, the battery, git status, a PR's checks, an AI provider's usage. A
screenshot or recording of such a widget normally depends on what happens to
be installed, signed in and true on the machine taking it. Fixtures remove
that dependency. With `SUPERLINE_FIXTURES` set, the widgets draw canned values
instead, so any change to how a widget looks can be shown from any machine,
without `gh`, Claude Code or Codex signed in, without a battery, and without
network access.

Widgets that read their data from the shell integration (`cmd`, `exit_code`,
`last_cmd_duration`, `jobs`, `shell`, `error`) or from Claude Code's status
line (`claude_*`) need no fixtures: the shell or the piped JSON already
decides what they show.

## Using fixtures

`SUPERLINE_FIXTURES` holds either a path to a JSON file or the JSON itself:

```bash
SUPERLINE_FIXTURES=examples/fixtures.json superline show fish
SUPERLINE_FIXTURES='{"hostname":"devbox","battery":{"percentage":8,"state":"discharging"}}' superline show fish
```

Each key names one backend, and its value is what that backend reports once
parsed, the same shape the widget would otherwise read from its cache.
[`examples/fixtures.json`](../examples/fixtures.json) names every backend and
is checked by the tests, so start from a copy of it.

- A backend that the fixtures name never runs: no CLI is started, no request
  is made, and nothing is read from or written to the cache, so the value is
  on the very first prompt.
- A backend that the fixtures do not name runs as usual. The two exceptions
  are the update check and auto-upgrade, which stay off unless named, so a
  demo never phones home or replaces the binary.
- A value that does not parse is reported on stderr, naming the key, and the
  widget draws as if the backend had nothing to report.

### Machine probes

| Key | Widgets | Value |
| --- | --- | --- |
| `clock` | `time`, `ai_usage` reset countdowns | `"2026-01-01T09:41:00Z"` (RFC 3339), or `"2026-01-01T09:41:00"` for a local time |
| `username` | `user` | `"demo"` |
| `root` | `user`, `cmd` | `true` or `false` |
| `ssh` | `Hostname::show_on_remote_shell` and `Username::show_on_remote_shell` (library API) | `true` or `false` |
| `hostname` | `hostname` | `"devbox"` |
| `os` | `os` | `"macos"`, `"linux"`, `"windows"`, `"freebsd"`, ... |
| `local_ip` | `local_ip` | `"192.168.1.20"` |
| `battery` | `battery` | `{"percentage": 8.0, "state": "discharging"}`; the state is `charging`, `discharging`, `full`, `empty` or `unknown` |
| `memory` | `memory_usage` | `{"total", "available", "swap_total", "swap_available"}` in bytes |
| `read_only` | `read_only` | `true` or `false` |

`null` means the machine has nothing to report (no battery, no network
address), which hides the widget.

### Cached lookups

| Key | Widgets | Value |
| --- | --- | --- |
| `git` | `git`, `git_remote` | counts (`ahead`, `behind`, `staged`, `non_staged`, `untracked`, `conflicted`), `branch_name`, `remote`, `remote_url`, `worktrees`, `worktree_index`; missing fields are zero |
| `diff` | `diff` | `{"added": 42, "removed": 7}` |
| `pr` | `pr`, `pr_checks`, `pr_diff` | by branch: `number`, `url`, `state`, and optionally `checks`, `review`, `diff`, `check_runs`; `null` for no PR |
| `usage` | `ai_usage` | by provider (`claude`, `codex`): `session`, `weekly`, `fable`, `credits`, `session_resets_at` (Unix seconds), `no_session_limit`, `logged_out` |
| `python` | `python` | the version, `"3.12.4"` |
| `kubernetes` | `kubernetes` | `{"name": "prod", "namespace": "web"}`, or `null` for no current context |
| `sudo` | `sudo` | `true` when credentials are cached |
| `update` | the update notice | `{"version": "v1.2.3", "url": "..."}` |
| `auto-upgrade` | the auto-upgrade notice | see `AutoUpgradeOutcome` in `src/upgrade.rs` |

`pr` and `usage` are objects keyed by branch and provider, where `"*"`
answers for any key not listed. Any lookup can also be held in its other
states, to show what a widget draws while it waits or when it cannot run:

```json
{ "usage": { "claude": { "$lookup": "loading" }, "codex": { "$lookup": "unavailable" } } }
```

Widgets that work from files in the current directory (`cwd`, the `git`
branch, `cargo`, `node`, `java`, the `python` virtualenv) read those files as
usual, so a demo creates them. The `git` widgets only need a `.git/HEAD`
naming the branch once `git` itself is fixtured; no `git` binary is involved.

Timestamps such as `session_resets_at` are absolute, so pair them with a
`clock` for countdowns that read the same on every run, and set `TZ` when the
`time` widget is in the shot.

## Recording a demo

Every PR that changes what a widget shows should carry a screenshot or
recording of it, made with fixtures so that anyone can reproduce it.
[`scripts/demo/record.sh`](../scripts/demo/record.sh) renders a config,
fixtures and a [VHS](https://github.com/charmbracelet/vhs) tape in an isolated
home with the branch-local build; see the comment at its top and the worked
example in `scripts/demo/examples/`. The website screenshots
(`scripts/site-screenshots/`) are made the same way, from
`scripts/site-screenshots/fixtures.json`, and the terminal snapshot rig takes
`--fixtures` (see [terminal-snapshots.md](terminal-snapshots.md)).

## Adding a backend

A new widget that reads anything outside the shell integration gets it
through `src/backend.rs`, so it can be fixtured from day one:

- a quick, synchronous question about the machine implements
  `backend::Probe` and is read with `backend::probe`;
- a slow one implements `cache::Source` as usual (see `AGENTS.md`), and
  overrides `Source::fixture_key` when its lookups differ by something an
  author can name, such as a branch or a provider. One that phones home or
  changes the installation sets `Source::LIVE_WITH_FIXTURES` to `false`.

Then add the key to `examples/fixtures.json` and to the tables above, and
cover the widget in `tests/backend_fixtures.rs`.
