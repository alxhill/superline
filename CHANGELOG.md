# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.22.0](https://github.com/alxhill/superline/compare/v0.21.0...v0.22.0) - 2026-09-25

### Added

- add Kubernetes context widget ([#118](https://github.com/alxhill/superline/pull/118))

## [0.21.0](https://github.com/alxhill/superline/compare/v0.20.2...v0.21.0) - 2026-09-25

### Added

- enable auto-upgrade by default ([#154](https://github.com/alxhill/superline/pull/154))
- download releases in-process with rustls, and cross-compile Linux builds with zigbuild ([#152](https://github.com/alxhill/superline/pull/152))
- auto-upgrade prebuilt binaries in the background with update.auto ([#148](https://github.com/alxhill/superline/pull/148))
- add superline upgrade to install the latest prebuilt release ([#147](https://github.com/alxhill/superline/pull/147))
- publish static ARM Linux builds for the Raspberry Pi ([#150](https://github.com/alxhill/superline/pull/150))

### Fixed

- deflake the Windows CI tests ([#153](https://github.com/alxhill/superline/pull/153))

## [0.20.2](https://github.com/alxhill/superline/compare/v0.20.1...v0.20.2) - 2026-09-24

### Fixed

- consistent detached HEAD branch name across git backends ([#143](https://github.com/alxhill/superline/pull/143))

## [0.20.1](https://github.com/alxhill/superline/compare/v0.20.0...v0.20.1) - 2026-09-24

### Fixed

- stop detached refreshes holding the Windows prompt pipe open ([#144](https://github.com/alxhill/superline/pull/144))

## [0.20.0](https://github.com/alxhill/superline/compare/v0.19.2...v0.20.0) - 2026-09-24

### Added

- [**breaking**] remove the angle_line separator ([#135](https://github.com/alxhill/superline/pull/135))

### Fixed

- set up the bash prompt on bash 3.2 and don't panic when init's stdout is closed ([#142](https://github.com/alxhill/superline/pull/142))
- count wide characters as two columns when aligning prompts ([#141](https://github.com/alxhill/superline/pull/141))
- read cmd failed_fg and failed_bg from the matching theme keys ([#137](https://github.com/alxhill/superline/pull/137))

### Other

- terminal snapshot tests from config.json + VHS tape cases ([#140](https://github.com/alxhill/superline/pull/140))
- publish a static x86_64-unknown-linux-musl release binary ([#132](https://github.com/alxhill/superline/pull/132))
- space out stacked rows in the website screenshots ([#139](https://github.com/alxhill/superline/pull/139))
- list each widget's theme options in the configuration reference ([#138](https://github.com/alxhill/superline/pull/138))

## [0.19.2](https://github.com/alxhill/superline/compare/v0.19.1...v0.19.2) - 2026-09-23

### Other

- add a configuration reference page with generated examples ([#134](https://github.com/alxhill/superline/pull/134))
- add a GitHub Pages site with VHS-generated screenshots ([#133](https://github.com/alxhill/superline/pull/133))
- add visual terminal snapshots ([#128](https://github.com/alxhill/superline/pull/128))

## [0.19.1](https://github.com/alxhill/superline/compare/v0.19.0...v0.19.1) - 2026-09-21

### Other

- *(git)* spawn fewer git processes and make auto mode smarter ([#129](https://github.com/alxhill/superline/pull/129))

## [0.19.0](https://github.com/alxhill/superline/compare/v0.18.2...v0.19.0) - 2026-09-20

### Added

- add a space after the OS symbol and a zero-character none separator ([#126](https://github.com/alxhill/superline/pull/126))

## [0.18.2](https://github.com/alxhill/superline/compare/v0.18.1...v0.18.2) - 2026-09-20

### Fixed

- *(jobs)* always show the count and skip stopped jobs ([#124](https://github.com/alxhill/superline/pull/124))

## [0.18.1](https://github.com/alxhill/superline/compare/v0.18.0...v0.18.1) - 2026-09-20

### Fixed

- *(jobs)* separate the cog glyph from the job count so they stop overlapping ([#122](https://github.com/alxhill/superline/pull/122))

## [0.18.0](https://github.com/alxhill/superline/compare/v0.17.0...v0.18.0) - 2026-09-20

### Added

- add collection of commonly used widgets ([#117](https://github.com/alxhill/superline/pull/117))

## [0.17.0](https://github.com/alxhill/superline/compare/v0.16.0...v0.17.0) - 2026-09-20

### Added

- *(update)* print a once-a-day notice above the prompt when a newer release is out ([#112](https://github.com/alxhill/superline/pull/112))

### Fixed

- *(cache)* give each cache writer its own temp file so concurrent refreshes never expose an empty entry ([#115](https://github.com/alxhill/superline/pull/115))

## [0.16.0](https://github.com/alxhill/superline/compare/v0.15.2...v0.16.0) - 2026-09-19

### Added

- *(git)* [**breaking**] pick the status backend at runtime and drop libgit2 ([#110](https://github.com/alxhill/superline/pull/110))
- *(git)* render a detached HEAD at a branch tip as '<hash> -> branch' ([#109](https://github.com/alxhill/superline/pull/109))

### Other

- *(cache)* run a timed refresh in-process instead of re-executing the binary ([#103](https://github.com/alxhill/superline/pull/103))

## [0.15.2](https://github.com/alxhill/superline/compare/v0.15.1...v0.15.2) - 2026-09-19

### Added

- *(git)* link the GitHub logo to the repository web page ([#107](https://github.com/alxhill/superline/pull/107))
- *(debug)* report per-module and cache timings with SUPERLINE_DEBUG=1 ([#100](https://github.com/alxhill/superline/pull/100))

### Other

- *(pr)* read the branch from HEAD instead of shelling out to git ([#102](https://github.com/alxhill/superline/pull/102))

## [0.15.1](https://github.com/alxhill/superline/compare/v0.15.0...v0.15.1) - 2026-09-19

### Added

- *(git)* show branch@hash when HEAD is detached at a branch tip ([#101](https://github.com/alxhill/superline/pull/101))

## [0.15.0](https://github.com/alxhill/superline/compare/v0.14.0...v0.15.0) - 2026-09-19

### Added

- *(config)* rename the nvm and python_env modules to node and python ([#97](https://github.com/alxhill/superline/pull/97))

## [0.14.0](https://github.com/alxhill/superline/compare/v0.13.1...v0.14.0) - 2026-09-18

### Added

- *(python_env)* fetch the venv interpreter version through the shared cache ([#95](https://github.com/alxhill/superline/pull/95))

### Other

- *(cache)* generic cached lookups with background refresh ([#94](https://github.com/alxhill/superline/pull/94))
- auto-merge the release PR only when the merged PR is labelled autorelease ([#92](https://github.com/alxhill/superline/pull/92))

## [0.13.1](https://github.com/alxhill/superline/compare/v0.13.0...v0.13.1) - 2026-09-16

### Other

- restructure README with per-module sections and short config snippets ([#90](https://github.com/alxhill/superline/pull/90))

## [0.13.0](https://github.com/alxhill/superline/compare/v0.12.0...v0.13.0) - 2026-09-16

### Added

- *(usage)* redraw the bar display and add capped_bar and block variants ([#86](https://github.com/alxhill/superline/pull/86))

## [0.12.0](https://github.com/alxhill/superline/compare/v0.11.6...v0.12.0) - 2026-09-16

### Added

- *(lang)* add version/jdk display options to the language segments ([#87](https://github.com/alxhill/superline/pull/87))

## [0.11.6](https://github.com/alxhill/superline/compare/v0.11.5...v0.11.6) - 2026-09-14

### Fixed

- *(usage)* drop the underscore glyph from the sparkline ramp ([#84](https://github.com/alxhill/superline/pull/84))

## [0.11.5](https://github.com/alxhill/superline/compare/v0.11.4...v0.11.5) - 2026-09-14

### Added

- *(usage)* improve 0 and low-usage display ([#82](https://github.com/alxhill/superline/pull/82))

## [0.11.4](https://github.com/alxhill/superline/compare/v0.11.3...v0.11.4) - 2026-09-11

### Other

- cancel in-progress runs when newer commits land ([#79](https://github.com/alxhill/superline/pull/79))

## [0.11.3](https://github.com/alxhill/superline/compare/v0.11.2...v0.11.3) - 2026-09-11

### Added

- add nushell support ([#75](https://github.com/alxhill/superline/pull/75))
- prune stale caches ([#77](https://github.com/alxhill/superline/pull/77))

### Fixed

- *(usage)* show a logged-out marker instead of launching the Claude login flow ([#76](https://github.com/alxhill/superline/pull/76))

## [0.11.2](https://github.com/alxhill/superline/compare/v0.11.1...v0.11.2) - 2026-09-10

### Other

- update readme

## [0.11.1](https://github.com/alxhill/superline/compare/v0.11.0...v0.11.1) - 2026-09-10

### Added

- add clear-caches subcommand ([#72](https://github.com/alxhill/superline/pull/72))

### Fixed

- *(usage)* make the ai_usage widget work on Windows ([#71](https://github.com/alxhill/superline/pull/71))

## [0.11.0](https://github.com/alxhill/superline/compare/v0.10.5...v0.11.0) - 2026-09-09

### Added

- *(usage)* show session reset time ([#69](https://github.com/alxhill/superline/pull/69))

## [0.10.5](https://github.com/alxhill/superline/compare/v0.10.4...v0.10.5) - 2026-09-08

### Added

- support cargo binstall with prebuilt release binaries ([#67](https://github.com/alxhill/superline/pull/67))

## [0.10.4](https://github.com/alxhill/superline/compare/v0.10.3...v0.10.4) - 2026-09-08

### Fixed

- *(usage)* use a space instead of a square for 0% in sparkline display ([#65](https://github.com/alxhill/superline/pull/65))

## [0.10.3](https://github.com/alxhill/superline/compare/v0.10.2...v0.10.3) - 2026-09-08

### Fixed

- *(usage)* render an empty square for 0% in sparkline display ([#63](https://github.com/alxhill/superline/pull/63))

## [0.10.2](https://github.com/alxhill/superline/compare/v0.10.1...v0.10.2) - 2026-09-08

### Other

- update agents

## [0.10.1](https://github.com/alxhill/superline/compare/v0.10.0...v0.10.1) - 2026-09-08

### Added

- *(usage)* accept aliases for ai_usage display styles ([#60](https://github.com/alxhill/superline/pull/60))

## [0.10.0](https://github.com/alxhill/superline/compare/v0.9.3...v0.10.0) - 2026-09-07

### Added

- *(usage)* show claude and codex usage credits ([#58](https://github.com/alxhill/superline/pull/58))

## [0.9.3](https://github.com/alxhill/superline/compare/v0.9.2...v0.9.3) - 2026-09-04

### Added

- add homebrew installation ([#56](https://github.com/alxhill/superline/pull/56))

## [0.9.2](https://github.com/alxhill/superline/compare/v0.9.1...v0.9.2) - 2026-09-04

### Other

- queue the release PR to auto-merge once CI passes ([#55](https://github.com/alxhill/superline/pull/55))
- open the release PR with a PAT so its checks run ([#53](https://github.com/alxhill/superline/pull/53))

## [0.9.1](https://github.com/alxhill/superline/compare/v0.9.0...v0.9.1) - 2026-09-04

### Added

- *(config)* show claude and codex usage in the default config ([#49](https://github.com/alxhill/superline/pull/49))

## [0.9.0](https://github.com/alxhill/superline/compare/v0.8.3...v0.9.0) - 2026-09-03

### Added

- *(usage)* add fable option to the Claude usage widget ([#47](https://github.com/alxhill/superline/pull/47))

### Fixed

- *(usage)* read codex rate limits over the app-server protocol ([#46](https://github.com/alxhill/superline/pull/46))

## [0.8.3](https://github.com/alxhill/superline/compare/v0.8.2...v0.8.3) - 2026-09-02

### Added

- add claude and codex usage widget ([#44](https://github.com/alxhill/superline/pull/44))

## [0.8.2](https://github.com/alxhill/superline/compare/v0.8.1...v0.8.2) - 2026-08-21

### Fixed

- *(git)* show the github logo whenever the repo has a remote ([#42](https://github.com/alxhill/superline/pull/42))

## [0.8.1](https://github.com/alxhill/superline/compare/v0.8.0...v0.8.1) - 2026-08-20

### Fixed

- *(pr)* show closed and merged drafts in their terminal state ([#40](https://github.com/alxhill/superline/pull/40))

## [0.8.0](https://github.com/alxhill/superline/compare/v0.7.0...v0.8.0) - 2026-08-19

### Added

- *(mise)* read java, node, python and rust versions from mise configs ([#38](https://github.com/alxhill/superline/pull/38))

### Added

- *(mise)* read java, node, python and rust versions from mise configs, marked
  with a mise indicator. The `sdkman` segment is now named `java` (the old name
  still parses).

## [0.7.0](https://github.com/alxhill/superline/compare/v0.6.2...v0.7.0) - 2026-07-24

### Added

- *(git)* make gitoxide the default backend ([#37](https://github.com/alxhill/superline/pull/37))
- *(git)* make status timeout configurable ([#35](https://github.com/alxhill/superline/pull/35))

## [0.6.2](https://github.com/alxhill/superline/compare/v0.6.1...v0.6.2) - 2026-07-22

### Fixed

- *(shell)* preserve powershell prompt after ctrl-c ([#33](https://github.com/alxhill/superline/pull/33))

### Fixed

- *(shell)* keep the Windows PowerShell prompt renderer alive when a Ctrl-C
  event interrupts the preceding native command, preventing PowerShell from
  falling back to `PS>`.

## [0.6.1](https://github.com/alxhill/superline/compare/v0.6.0...v0.6.1) - 2026-07-21

### Fixed

- *(git)* cache status after 100ms timeout ([#31](https://github.com/alxhill/superline/pull/31))

## [0.6.0](https://github.com/alxhill/superline/compare/v0.5.5...v0.6.0) - 2026-07-06

### Fixed

- improve config and theme error fallback ([#29](https://github.com/alxhill/superline/pull/29))

## [0.5.5](https://github.com/alxhill/superline/compare/v0.5.4...v0.5.5) - 2026-06-26

### Added

- *(config)* sync default config with recommended setup ([#25](https://github.com/alxhill/superline/pull/25))

### Fixed

- *(cwd)* show current dir under Git Bash on Windows ([#27](https://github.com/alxhill/superline/pull/27))

### Fixed

- *(cwd)* show the current directory under Git Bash on Windows. The native
  Windows binary was reading the MSYS-style `$PWD` (e.g. `/c/Users/alex`) and
  splitting it on `\`, so the whole path collapsed into one segment that the
  leading `skip(1)` discarded, leaving the module empty. On Windows the cwd is
  now always taken from the real working directory, which yields a proper
  `C:\...` path.

## [0.5.4](https://github.com/alxhill/superline/compare/v0.5.3...v0.5.4) - 2026-06-23

### Added

- *(shell)* fix powershell utf8 rendering ([#23](https://github.com/alxhill/superline/pull/23))
- *(git)* add gitoxide backend ([#21](https://github.com/alxhill/superline/pull/21))

### Fixed

- filter out empty dirs from gitoxide status ([#24](https://github.com/alxhill/superline/pull/24))
- *(install)* target the PowerShell edition install was run from ([#22](https://github.com/alxhill/superline/pull/22))
- *(platform)* link advapi32 on Windows for the libgit2 backend ([#19](https://github.com/alxhill/superline/pull/19))

### Fixed

- *(shell)* set `[Console]::OutputEncoding` to UTF-8 in the PowerShell init so
  Nerd Font glyphs and powerline separators aren't mangled into mojibake (e.g.
  `εé░`) when PowerShell decodes superline's output using the legacy OEM code page.

## [0.5.3](https://github.com/alxhill/superline/compare/v0.5.2...v0.5.3) - 2026-06-16

### Added

- *(platform)* add Windows compatibility ([#18](https://github.com/alxhill/superline/pull/18))
- *(shell)* add PowerShell (pwsh) support ([#17](https://github.com/alxhill/superline/pull/17))

### Other

- document custom theme JSON format ([#15](https://github.com/alxhill/superline/pull/15))

## [0.5.2](https://github.com/alxhill/superline/compare/v0.5.1...v0.5.2) - 2026-06-15

### Other

- use absolute image URLs in README so they render on crates.io ([#13](https://github.com/alxhill/superline/pull/13))

## [0.5.1](https://github.com/alxhill/superline/compare/v0.5.0...v0.5.1) - 2026-06-15

### Added

- *(pr)* only show status indicator for in-progress PRs ([#11](https://github.com/alxhill/superline/pull/11))

### Other

- add release-plz and conventional-commit PR title check ([#7](https://github.com/alxhill/superline/pull/7))
- install from crates.io and add a crates.io badge ([#10](https://github.com/alxhill/superline/pull/10))
