## Unreleased

### Breaking Changes

- **file commands**: the `@file..verb` form is gone — write `byk @file <verb>` (and `byk @@file <verb>`) instead. The separator is now a space, so it no longer collides with the precise alias syntax `@file.key`. `byk @file` on its own, `-h`, or `--help` prints that file's command help
- **file help**: the `help` file command is gone — use `-h` / `--help` (`byk @file -h`, `byk @file init -h`). File-command help now mirrors `byk -h`: the index names the local/global file and shows `FilePath` (with `(not created yet)` / `(link)` markers), and every command's help repeats the target path above Usage with its description first
- **`clone`**: the `-jd` / `--jsdelivr` source is gone; read GitHub through a CDN with `-gh <spec> jsdelivr` instead

### Features

- **`$paths`**: now inheritable like `$cwd` / `$mode` — writable on a file, group, or alias object; a child value replaces the inherited list entirely (no merging), and `[]` clears it so a single alias can fall back to the system `PATH`
- **`clone -gh`**: takes an optional second value naming the CDN — `raw` (default), `jsdelivr`, or `ghproxy` — so more mirrors can be added without new flags
- **`$deps`**: when `base` is a ghproxy address, absolute GitHub downloads (release assets included) reuse the same proxy; relative entries are never wrapped twice, and jsdelivr remains repository-files-only
- **`$deps`**: curl's native progress bar is shown on a terminal; non-TTY output stays quiet with unchanged error text
- **network**: downloads can be forced direct with `BYK_USE_PROXY=false`; otherwise they follow `BYK_PROXY`, then existing proxy environment variables, then the static Windows system proxy. On Windows the bypass list becomes `no_proxy`, while PAC/WPAD only warn. The proxy is injected into curl child processes alone, never the alias environment or pip/npm
- **`@@@` system files**: a third file domain for byk's own files inside the CLI home. `@@@` tokens are claimed before the alias dot rule, so they never fall through to alias lookup — `@@@config.set` now reports `Invalid system file` (it used to be `Invalid file name`), and an unknown name lists the ones that exist
- **`@@@config`**: settings live in `~/.byk/config.json` and are driven by verbs — `list`, `get`, `set`, `unset`; with no verb it prints every key with its current value, default, and description. Values are checked against a registry, and the file is read leniently: missing or broken content falls back to defaults with one warning instead of failing
- **`@@@tap`**: a new system file managing the tap repositories under `~/.byk/tap/` — `add <user>/<repo>[@<ref>]` clones it shallowly with git (falling back to a curl tarball when git is missing), checks the repository root for `byk.tap.json`, lands it in `tap/<user>/<repo>`, and records it in `tap/index.json`; `remove` unrecords and deletes, `list` shows what is installed. Only `@@@tap` reads that index, so `clone --tap` is unchanged
- **`banner`**: `@@@config set banner auto|always|never` (default `auto`) controls the decorative box above the help output; `auto` hides it when stdout is not a terminal, so `byk -h | head` and editor plugins stay clean. The scan-problem line is printed regardless
- **`--paths`**: adds a `Config:` line naming the settings file

## v0.8.2 (2026-11-20)

### Features

- **`$deps`**: alias files can declare dependencies — files, pip packages, and npm packages — installed into the file's own dedicated directory; `@file..deps install|status|uninstall` manages them, and `..clone -d` installs right after cloning

- **`$clone`**: new inheritable field that keeps a file, group, or alias out of `..clone` copies while it stays fully usable locally; it does not affect the merged space, and a clone that would leave nothing is refused

- **`__api__ expand`**: new `--no-builtins` flag expands only the file's `$var` declarations and leaves `byk_dir` / `work_dir` / `file_dir` / `deps_dir` unset

- **`..clone` / `..link` / `..init --link`**: bare local paths are read literally, falling back to `.byk.json`; `.`, `..`, a trailing `/`, and directory names read that directory's `.byk.json`; names that cannot be alias files (`hello.txt`) are never suffixed

- **`..clone --tap`**: new GitHub-only source that resolves a key through `byk.tap.json`; the full form prefers a read-only local index under `~/.byk/tap/` and falls back to the repository root, a bare `--tap <key>` searches all local indexes and accepts only a unique match, and the target is loaded as one hop

## v0.8.1 (2026-11-14)

### Features

- **variables**: `${work_dir}` expands to the invocation directory, unaffected by an inherited `$cwd`; built-ins expand to absolute paths, so it can go straight into `$cwd`
- **`__api__ paths`**: JSON adds `workdir` and `aliasFiles` (global first, then local, each sorted by key, deduplicated when both scopes are the same directory); scalar lookup now also accepts `workdir` alongside `root` / `alias`
- **`__api__ file`**: read-only request resolving a file key (`@name` / `@@name` / `@` / `@@`) to `{key, scope, path, dir}` without checking existence, reading contents, or following links; `--list` returns every alias file in the same shape
- **`__api__ expand`**: expands file-level `$var` and built-ins in any JSON file; no alias validation, link following, or execution
- **`__api__`**: the no-arg manifest now includes `version`

### Bug Fixes

- **`--check`**: reports invalid file names and broken symlinks; broken symlinks still show in `--paths` / `file --list`, and `scanned` counts candidate `*.byk.json` entries
- **exact execution**: a missing file and an unloadable one are distinguished (reason plus a `byk --check` hint); a `$`-prefixed path segment is rejected as reserved metadata instead of executing `$cmd` / `$cwd`
- **help**: the alias list omits the resolved `$cwd` when a `$description` is present
- **file commands**: `..destroy` / `..clear` confirmation prompts handle CJK backspace in terminals
- **file names**: accept CJK ideographs in Ext A–I and the compatibility blocks, not just U+4E00–U+9FFF

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