## Unreleased

### Features

- **variables**: `${work_dir}` expands to the directory `byk` was invoked from, unaffected by inherited `$cwd`; built-ins expand to absolute paths, so it can go directly into `$cwd` to escape a file-level directory
- **`__api__ paths`**: the JSON now also carries `workdir` and `aliasFiles`, a path-only list of alias files enumerated by filename (global first, then local, sorted by key)
- **`__api__ paths`**: `<name>` now also accepts `workdir`, so every JSON directory field has a scalar form (`root` / `alias` / `workdir`)
- **`__api__ file`**: new read-only request that resolves one alias file key (`@name` / `@@name` / `@` / `@@`) to `{key, scope, path, dir}` without checking existence, reading contents, or following links; `--list` returns every alias file as an array of the same objects
- **`__api__ expand`**: expands file-level `$var` and built-in variables in any JSON file and prints the result; no alias-structure validation, no link following, no execution

### Bug Fixes

- **diagnostics**: `--check` now reports invalid file names and broken symlinks; broken symlinks still appear in `--paths` / `file --list`, and `scanned` counts the candidate `*.byk.json` entries encountered
- **exact execution**: distinguishes a missing file from a file that exists but cannot be loaded, with the reason and a `byk --check` hint; a path segment starting with `$` is rejected as a reserved field instead of executing `$cmd` / `$cwd` and other metadata as aliases

## v0.8.0 (2026-11-13)

### Breaking Changes

- **`$priority`**: negative values no longer keep a file out of the merged namespace; they are treated as `0`

### Features

- **`$private`**: new inheritable field that keeps a file, group, or alias out of the merged space while exact calls still work; it replaces `$priority: -1`
- **file commands**: alias files now carry reserved commands addressed as `@file..<verb>` / `@@file..<verb>` — `init`, `destroy`, `clear`, `set`, `remove`, `json`, `path`, `rename`, `clone`, `link`, `help` — to create, edit, clone, link, and inspect the file itself
- **completion**: file-scope keys (`@file` / `@@file`) are offered only after typing `@` / `@@`; an empty Tab now lists merged aliases only
- **completion**: file keys now carry their alias file path as the description under `--with-descriptions`; file commands and their options are completed after `..`
- **`--check`**: diagnostic report; unreadable files, invalid JSON, bad metadata types, and link problems are summarized in the help banner instead of being printed during normal runs
- **`--paths`**: now also prints the working directory and lists alias files in the global alias directory and the current working directory; file names are validated, contents are not parsed
- **`__api__ check`**: scan diagnostics as JSON for editor plugins; problems appear in the payload and keep the exit code at `0`, while `--check` keeps the human-readable report and exits `1`
- **`-v`**: a symlinked executable now shows its target after the install path
- **`$recursive`**: new non-inheritable field that lets an alias re-enter itself on the call chain; `BYK_ALLOW_RECURSION=1` allows it for one run, while chains deeper than 32 levels are always stopped

### Bug Fixes

- **execution**: a self-referencing alias is detected when the chain loops back and stops there, with the whole chain in the error (`@dev.build -> @dev.release -> @dev.build`)
- **execution**: substituted values are quoted, so an argument containing spaces, `;`, `*`, or a backslash reaches the command as one literal argument
- **completion**: nested aliases are no longer completed after a space; only the first word position is completed

### Performance

- **execution**: `@file.key` / `@@file.key` parse only the target alias file instead of scanning and merging every alias file
- **`--check` / completion**: scanning no longer builds the merged alias tree when only files or diagnostics are needed
- **completion**: `@file.` completes from the target file only, and file-key descriptions use a name-only directory listing instead of parsing every alias file

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