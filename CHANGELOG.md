# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Fixed

- *(shell)* keep the Windows PowerShell prompt renderer alive when a Ctrl-C
  event interrupts the preceding native command, preventing PowerShell from
  falling back to `PS>`.

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
