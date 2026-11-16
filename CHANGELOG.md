## Unreleased

### Bug Fixes

- **`--check`**: `Hint:` continuation lines in a diagnostic now indent to the message column. They used to start at column zero and read like a second diagnostic for the same file

- **`$deps`**: an `extract` entry without `to` (unpacking into the dependency root) was treated as already installed, so `..deps install` skipped the download and `..deps status` reported `ok` over an empty directory; root extraction now counts as installed only when the directory actually has content, and is repaired when it does not
- **`$deps`**: install's summary line says `Downloaded` for both transports and counts the local copies in parentheses (`Downloaded 3 files into … (1 local)`; no parentheses when nothing is local) — `$deps.files` only fetches files, the verb is about fetching, and `$deps` will grow other dependency kinds rather than calling them all an install. `..deps status` likewise reports `N present` instead of `N installed`, counting what is on disk
- **VS Code extension**: `$deps.files[].src` in the JSON schema now accepts values starting with `${...}` — variables are expanded before the http(s) check, so the schema was flagging valid declarations
- **`$deps`**: root extraction is rejected when combined with other entries — reinstalling replaces the whole directory and would silently drop them

### Features

- **`$deps.base`**: new `base` field, and `$deps.files[].src` now accepts relative paths. `base` is a **directory** the relative `src` is joined onto (in URLs `../` climbs as far as it wants, stopping at the host root; local paths are normalised lexically the same way). On disk `base` must be absolute: a remote clone writes an absolute http(s) URL, a local clone writes a local absolute path — an alias file lives in `~/.byk/alias/` and carries no location of its own, so a relative value cannot be resolved there. Freezing happens only in `..clone`: the source file's directory when `base` is absent, resolved against the source file when relative (`.`, `../`, `sub/dir`), kept as-is when already absolute with only the trailing `/` added. Copies, `..link` and further clones then anchor to the same place instead of drifting. The manifest records the resolved absolute address, so editing `base` changes the source: the next install classifies the affected assets as `outdated`, making `@v1` → `@v2` a one-line change. `..deps status` prints a `Base:` line. Documents without `$deps` are still byte-for-byte copies, and an already correct `base` is not rewritten. `base` is meant to be a literal written by a program: `${...}` variables are rejected (a clone has no variable table, and guessing would bake a bogus choice into the file), so clone fails instead of writing a half-broken file. A `/sub`-style base is rejected on Windows too: with no drive letter it resolves against the current drive, so only relative paths and drive-letter absolutes (`C:/share/`) are accepted

- **`$deps` from local sources**: `base` can also be a local absolute directory; relative `src` is then joined as a path and copied into the dedicated directory with `fs::copy` (same temp-file-then-rename dance, so a failure never leaves half a file). `extract` entries still go through `tar`, and `curl` is not involved at all — a local test loop can build an artefact and have `..clone -d` or `..deps i` pick it up, even with no `curl` on `PATH`. The boundary is structural: a remote clone only ever writes the URL form and rejects a local-path `base`, so **an alias file taken from the network cannot read local files**. Local entries are compared by source mtime: edit the source, rerun, and it is reinstalled (`--force` rebuilds unconditionally); a missing source is reported with its path instead of a quiet "up to date". A local `base` is not portable — it names an absolute path on this machine

- **`$deps` for local files**: `@<file>..deps` no longer refuses local scope. Local assets land in `<file-dir>/.byk/deps/@<file-name>/`, next to the alias file, with a self-ignoring `.gitignore` (`*`) so downloads never reach `git status` — a repository can ship a `.byk.json` that pins its own tooling without touching `~/.byk`. `..clone -d`, `..destroy`, `..rename`, `${deps_dir}` and `--check` all follow the same rules for both scopes, and the two roots share one layout (`<root>/deps/<file-key>`), so `~/.byk/deps/@@hello` and `<project>/.byk/deps/@hello` need no separate code paths

- **`$deps`**: global alias files can declare asset dependencies via `$deps.files` (string shorthand or `{src, to, name, extract, exec}` objects); declared assets land in the file's own directory `~/.byk/deps/<key>/`, guarded against path escapes
- **`${deps_dir}`**: new built-in variable pointing at the file's dependency directory; only injected for global alias files, left as-is elsewhere
- **file commands**: `@@<file>..deps install` downloads declared assets (delegating to system `curl` and `tar`, no new dependencies), printing each asset's source and destination as it goes and installing without a second confirmation; `@@<file>..deps uninstall` removes the directory and still asks first. `..rename` and `..destroy` now move/remove the dependency directory, and `..link` warns when retargeting a file that still has one
- **file commands**: `..clone` takes `-d` / `--deps` to install the cloned file's `$deps` in the same call — no second confirmation, progress printed, and a failed install leaves the clone in place for a retry; options may now sit before or after the source and combine (`-gh user/repo@v1/tool -d`)
- **`$deps` sync**: `..deps install` now converges the directory to the declaration instead of appending. A manifest (`.byk-deps.json`) records what was installed and from where, so unchanged assets are skipped without touching the network, changed URLs are replaced atomically, entries dropped from the declaration are deleted, manually deleted files are repaired, and an `exec`-only change is a plain `chmod`. Directories from older versions are adopted rather than refused, files byk did not install are never deleted (only reported), `--force` rebuilds everything, and reinstalling an `extract` entry replaces the whole directory so files dropped from a newer archive do not linger
- **`..deps status`**: reports `ok` / `missing` / `outdated` / `stale` using the same classification as install, and `--check` validates the declaration itself without looking at what is installed
- **file commands**: `..deps` accepts unambiguous prefixes (`i` / `u` / `s`) and gains a `status` subcommand; bare `..deps` prints what is declared and what is installed to stdout
- **`--check`**: validates the `$deps` structure

- **`__api__ expand`**: new `--no-builtins` flag expands only the file's `$var` declarations and leaves `byk_dir` / `work_dir` / `file_dir` unset

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