# BYK

VS Code integration for [byk](https://github.com/fcbyk/byk) — run aliases from `*.byk.json` files without leaving the editor.

## Features

- **Right-click to run** — execute the alias under the cursor, from the editor or the Explorer
- **Quick Pick** — search every alias in a file via the command palette
- **JSON validation** — validates `*.byk.json` against the alias schema
- **Path completion** — completes file paths in `$cwd` values and `$paths` entries
- **Task provider** — registers `byk` as a VS Code task type, so output lands in the task panel
- **Bundled binary** — ships with platform binaries and uses them by default; two settings reorder the lookup when you need a system or development build instead

## Usage

**Step 1** — Create a `*.byk.json` file in your project:

```jsonc
{
  "dev": "vite",
  "build": "vite build"
}
```

**Step 2** — Open the file and **right-click anywhere → Run Alias** to run the alias at the cursor.

Alternatively, **right-click → Alias List** to search all aliases in the file through the Quick Pick panel. You can also right-click a `*.byk.json` file in the Explorer and pick **Alias List**.

The extension activates automatically when the workspace contains any `*.byk.json`.

### Supported filenames

| File               | Runs as                |
| ------------------ | ---------------------- |
| `run.byk.json`     | `byk @run.dev`         |
| `release.byk.json` | `byk @release.publish` |
| `.byk.json`        | `byk @.build`          |

The `@file.key` form calls the alias in that exact file, so a project file works even though it does not take part in global alias merging.

## Settings

### Choosing which `byk` runs

Two settings, one mechanism: both feed the `PATH` handed to every task, and the
first directory containing `byk` / `byk.exe` wins.

| Setting | Type | Default | Meaning |
| --- | --- | --- | --- |
| `byk.priorityPaths` | `string[]` | `[]` | **First priority.** Directories searched in array order, ahead of everything else. Absolute paths only, `~` is expanded, missing directories are skipped silently |
| `byk.secondarySource` | `bundled` \| `system` | `bundled` | **Second priority.** Which tier comes next when the directories above miss — the other one becomes the final fallback |

The resulting lookup order:

| `priorityPaths` | `secondarySource` | Order |
| --- | --- | --- |
| entries | `bundled` | `priorityPaths` → bundled → `PATH` |
| entries | `system` | `priorityPaths` → `PATH` → bundled |
| empty | `bundled` | bundled → `PATH` |
| empty | `system` | `PATH` → bundled |

`system` means your `PATH`, nothing more — the extension adds no install
locations of its own. To use `~/.byk/bin`, list it in `priorityPaths`:

```jsonc
{
  "byk.priorityPaths": ["~/dev/byk/target/release", "~/.byk/bin"],
  "byk.secondarySource": "bundled"
}
```

Note that the binary has to be named `byk` (`byk.exe` on Windows) — these are
directories on a search path, not pointers to individual files.

### Which binary am I actually running?

Run **BYK: Show Active byk** from the command palette. It prints the resolved
search order, marks the directory that won, and reports that binary's
`--version`. Prefer it over running `byk --version` in a terminal — the
terminal's `PATH` is not the one the extension hands to its tasks.

## Versioning

The extension is a shell around the CLI, so its version tracks the CLI's:

| Field | Meaning |
| --- | --- |
| `version` (`X.Y.Z`) | `X.Y` equals the bundled CLI's `major.minor`; `Z` increments independently for extension-only fixes |
| `bykBinaryVersion` | The exact CLI release bundled into this build |

`0.6.3` therefore means "the fourth extension build on the byk 0.6 line", while `bykBinaryVersion` says precisely which `0.6.x` binary it ships. When the CLI moves to a new `X.Y`, the extension resets `Z` to `0`.

`scripts/download-binary.mjs` picks the release tag from `bykBinaryVersion`, so packaging is reproducible — it never resolves `latest` at build time.

Packaging also cross-checks the repository's `Cargo.toml`, which is the source of truth for CLI releases: bundling a CLI **newer** than the one declared there fails the build (that tag cannot exist yet), while lagging **behind** only prints a hint naming the newer release.

## Learn more

- [byk CLI docs](https://cli.fcbyk.com)
- [Alias syntax](https://cli.fcbyk.com/cli/alias/start) — nested groups, `$cwd`, `$interactive`, placeholders, etc.
