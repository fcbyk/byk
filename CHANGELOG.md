## Unreleased

### Breaking Changes

- **`$priority`**: negative values no longer keep a file out of the merged namespace; they are treated as `0`

### Features

- **`$private`**: new inheritable field that keeps a file, group, or alias out of the merged space while exact calls still work; it replaces `$priority: -1`
- **`..set`**: add `--private` / `--no-private` for both the file root and a single alias
- **`--check`**: diagnostic report; unreadable files, invalid JSON, bad metadata types, and link problems are summarized in the help banner instead of being printed during normal runs

## v0.7.0 (2026-11-08)

### Breaking Changes

- **variables**: variable syntax changed from `$name` to `${name}` — bare `$xx` is now passed to the shell untouched
- **variables**: `${byk}` renamed to `${byk_dir}`, `$workdir` removed, new `${file_dir}` points to the alias file's directory
- **`$mode`**: `$interactive` / `$silent` booleans replaced by a three-state `$mode` (`normal` / `interactive` / `silent`)
- **variables**: values with whitespace are no longer auto-quoted — quote them yourself

### Features

- **`$mode`**: `silent` suppresses byk's hint lines; `interactive` keeps confirmation prompts; inheritable across files
- **variables**: `$var` declarations can override built-in names; `\${...}` passes through to the shell literally
- **link**: a `*.byk.json` root array with a single entry now points to another alias file (one level deep), so bundles can live beside their scripts
- **`__api__ paths`**: prints CLI directories as JSON or bare strings for shell scripts

### Bug Fixes

- **execution**: hint lines (source line, resolved command) now go to stderr and are skipped when stderr is not a terminal
- **`$paths`**: relative entries now resolve against the config file's directory, not the working directory
- **variables**: undefined `${NAME}` is passed through to the shell instead of being swallowed
- **variables**: circular references no longer cause infinite expansion

## v0.6.0 (2026-10-29)

### Breaking Changes

- **commands**: remove all built-in subcommands (`add`, `remove`, `show`); byk is now a pure alias executor
- **plugins**: remove plugin protocol and plugin installation/management
- **cache**: remove alias caching; changes to `*.byk.json` files take effect immediately
- **completion**: remove public `completion` subcommand; shell completion now uses the `__api__` interface
- **node / py**: remove Node.js (npm/pnpm) and Python (py-v/uv) initialization features

### Features

- **`__api__`**: add hidden built-in command `byk __api__` for shell completion script generation and candidate queries
- **`--paths`**: add `--paths` option to display CLI home and alias directory paths
- **`$var`**: alias files now support `$var` declarations with per-platform value resolution (linux/macos/windows)

## v0.5.1 (2026-07-13)

### Features

- **completion**: improve contextual tab completions
- **plugins**: skip persistence for download-to-workdir-only plugins
- **install**: add streaming download with progress bar

### Bug Fixes

- **plugins**: auto-create parent directories when saving files in add command
- **scripts**: replace ?? operator with PS 5.1 compatible null check in install.ps1

## v0.5.0 (2026-07-06)

### Breaking Changes

- **plugins**: py-module → python-m, py-script → python, py-bin → pip-bin
- **plugins**: unify all downloads into downloads / download-to-workdir / download-to-alias
- **plugins**: remove bin and bin-tar fields, use downloads with [tar] / [exe] prefix

### Features

- **plugins**: `pip` and `pip-keep`  accepts a single string, not just an array
- **plugins**: `$var` supports per-platform values
- **plugins**: add `alias` fields to plugin protocol

### Bug Fixes

- **Windows**: fix alias path display error
- **Windows**: fix alias execution error

### Tests

- add e2e smoke tests for basic binary availability

## v0.4.0 (2026-06-27)

### Breaking Changes

- **plugins**: remove `pip-e` field from plugin protocol and `-e` flag from add command
- **plugins**: extract `scripts` field from `commands`/`command`
- **add**: replace -b/--branch flag with @ syntax for branch selection
- **plugins**: flatten install.pip to pip, add pip-keep for shared deps
- **add**: resolve plugin key deterministically via $default

### Bug Fixes

- ensure py-script entries download regardless of path prefix
- uninstall_plugin fails when venv has no pip (uv mode)

### Features

- **plugins**: add verbose process logging for install and uninstall
- **plugins**: add bin and bin-tar support for platform-specific binary downloads
- **cli**: add compile-time platform detection for `-v` output
- **plugins**: add $var variable substitution to byk.json plugin protocol
- **plugins**: add `command` field for single-command plugins
- **plugins**: add relative path resolution and remove download field
- **add**: add --cdn flag to fetch GitHub plugins via jsDelivr
- **add**: extend --file to support remote URLs
- **show**: add paths subcommand and simplify overview
- **add**: add uv support alongside py-v for Python environment management
- **add**: add py-v feature for Python venv & pip aliases

### Refactor

- **plugins**: flatten plugins.pkg.json structure and improve plugin list display

## v0.3.0 (2026-06-23)

### Breaking Changes

- remove init/remove py-v, auto-create venv in `byk add` (#7)
- merge `init` subcommand into `add`
- replace --info option with `show` subcommand
- replace bykpy with native Rust plugin discovery，plugin protocol is operation-driven design 
- remove global py feature, keep only py-v

### Features

- **remove**: track installed packages and add `byk remove <key>` uninstall (#7)
- **alias**: add $description field for alias help display
- **alias**: add $paths field to alias files for PATH prepending

### Chores

- add VS Code extension from external repository
- add optional python sdk from external repository
- add py-bin、py-script、py-module example plugins

### Documentation

- add sdk documentation
- add CLI commands, plugin system, and plugin registry docs

## v0.2.0 (2026-06-14)

### Features

- add init/remove subcommands and zero-config startup (#4)
- remove --info latest version check (#1)

### Refactor

- simplify --info, remove dashboard
- improve alias cwd display
- make npm cache lazy, skip entire path when node-pkgs absent

### Bug Fixes

- clear stale cache when bykpy runtime is unavailable
- replace stdin read_line with rustyline for CJK-aware input (#2)

## v0.1.0 (2026-06-10)

Initial release.