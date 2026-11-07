## Unreleased

### Breaking Changes

- **`$mode`**: the boolean `$interactive` is gone, replaced by an inheritable three-state `$mode` (`normal` / `interactive` / `silent`); old files are not read for it — an alias that relied on `$interactive: true` loses its confirmation prompt and needs `"$mode": "interactive"`
- **vscode**: replace `useSystemBinary` / `binaryPath` with `priorityPaths` / `secondarySource` — the old pair was self-contradictory: `useSystemBinary: false` only skipped injecting `~/.byk/bin` and friends, while the system `PATH` still ranked ahead of the bundled binary, so the documented default never held
- **variables**: variable syntax is now `${name}`; bare `$xx` is no longer interpreted and is passed to the shell untouched — rewrite `$byk` as `${byk_dir}` and `$VAR` as `${VAR}`
- **variables**: `$workdir` is gone; the invocation directory is the shell's `$PWD`, which costs nothing to reach and cannot drift out of sync with the alias's own `$cwd`
- **variables**: all substitution now happens during the scan phase; there is no second expansion before execution
- **variables**: values containing whitespace are no longer auto-quoted — quote them yourself following shell rules

### Features

- **vscode**: `byk.priorityPaths` takes an array of directories searched ahead of everything else, in order, so several candidate builds can coexist
- **vscode**: `byk.secondarySource` (`bundled` | `system`) decides which tier follows the priority paths; the other one stays as the final fallback instead of being dropped
- **vscode**: add **BYK: Show Active byk** — prints the resolved search order, the directory that won, and that binary's `--version`, since a terminal's `PATH` is not the one tasks receive
- **variables**: built-in and `$var` variables share one table; a declaration with a built-in name overrides it, so explicitly written values always win
- **variables**: self-reference such as `"byk_dir": "${byk_dir}/tools"` resolves the right-hand side against the built-in value (`local byk_dir = $byk_dir/tools`)
- **variables**: `\${...}` hands the expression to the shell instead of byk, useful for printing a literal
- **`${byk_dir}`**: the built-in formerly known as `${byk}` — byk's resource root (`~/.byk`, not the binary directory); every built-in now ends in `_dir`, so the suffix alone says "this is a path"
- **`${file_dir}`**: the directory holding the alias file being scanned, so a global alias in `~/.byk/alias` can reach scripts that sit beside it; injected per file rather than shared, and still overridable from `$var`
- **variables**: variable names are limited to `[A-Za-z_][A-Za-z0-9_]*`; invalid names and the reserved `args` are reported on stderr
- **`$mode`**: one inheritable three-state property (`normal` / `interactive` / `silent`) replaces the two independent booleans — `silent` drops byk's own hint lines so `byk build > log.txt` and `byk build | grep error` carry only the child's output, while `interactive` keeps every prompt; the meaningless combination is now unrepresentable, and an alias under a file-level `interactive` reaches `silent` with a single word instead of turning a second switch off
- **`$mode`**: an unrecognised value prints a warning and falls back to `normal` instead of making the whole alias vanish from the namespace
- **`__api__ resolve`**: reports the authoritative `mode` alongside the derived `interactive` boolean, so an extension that only knows the boolean still refuses to run an interactive alias unattended
- **`__api__ paths`**: `paths` prints the two CLI directories as JSON for programs, while `paths root|alias` prints that one value as a bare string — `"$(byk __api__ paths root)/bin"` needs neither `jq` nor quote stripping, and the install layout stops being hardcoded in shell scripts

- **import**: a `*.byk.json` whose root is a one-entry array points at another alias file, so a bundle can live next to the scripts it runs instead of in `~/.byk/alias` — aliases, `$cwd`, `$paths`, `$var` and `$mode` all come from the target and relative paths resolve against the target's directory, while the `@file` key and default priority stay with the index; the same bundle therefore behaves identically whether it is scanned directly or reached through an index
- **import**: one level only — a target that is itself an array is an error, and a missing or broken target skips just that file rather than the whole run

### Bug Fixes

- **execution**: the hint lines move to stderr, so `byk build > log.txt` and `byk build | grep error` receive only the child process's output instead of the source line and the resolved command
- **execution**: the hint lines are skipped when stderr is not a terminal — CI runs, `2>` redirects and editor tasks stay quiet without every alias needing `$mode: "silent"`, and the check is deliberately on stderr so piping stdout does not mute the terminal
- **vscode**: the bundled binary was unreachable by default — it sat last in `PATH`, behind the system one, so any user with a `byk` on `PATH` always ran that copy instead of the pinned one
- **vscode**: an empty `PATH` segment is no longer emitted (POSIX reads it as the current directory), and `PATH` is written under the key name the platform actually uses
- **`$paths`**: relative entries now resolve against the config file's directory instead of the command's working directory, so an alias still finds its scripts when invoked from a subdirectory
- **variables**: undefined `${NAME}` is no longer swallowed by the argument-placeholder layer and now survives to the shell
- **variables**: circular or self-referential variables no longer balloon into repeated nested text; resolution uses a snapshot closure with self-reference freezing

### Chores

- **vscode**: version the extension off the CLI it bundles — `X.Y` mirrors the CLI's `major.minor` and `Z` is the extension's own revision, with `bykBinaryVersion` pinning the exact binary
- **vscode**: `download-binary.mjs` resolves its release tag from `bykBinaryVersion` instead of falling back to `latest`, making a packaged build reproducible
- **vscode**: `scripts/verify-version.mjs` fails the build when `version` and `bykBinaryVersion` disagree on `major.minor`, when the bundled CLI would move backwards, or when `bykBinaryVersion` points ahead of `Cargo.toml`; a bundle lagging behind `Cargo.toml` only prints a hint (`BYK_ALLOW_BINARY_DOWNGRADE=1` overrides a deliberate rollback)

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

## Breaking Changes

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

## Breaking Changes

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