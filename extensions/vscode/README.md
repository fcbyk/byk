# BYK

VS Code integration for [byk](https://github.com/fcbyk/byk) — run aliases from `*.byk.json` files without leaving the editor.

## Features

- **Right-click to run** — execute the alias under the cursor, from the editor or the Explorer
- **Quick Pick** — search every alias in a file via the command palette
- **JSON validation** — validates `*.byk.json` against the alias schema
- **Path completion** — completes file paths in `$cwd` values and `$paths` entries
- **Task provider** — registers `byk` as a VS Code task type, so output lands in the task panel
- **Bundled binary** — ships with platform binaries; can be pointed at a system install instead

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

| Setting               | Description                                           | Default |
| --------------------- | ----------------------------------------------------- | ------- |
| `byk.useSystemBinary` | Use the `byk` on `PATH` instead of the bundled binary | `false` |
| `byk.binaryPath`      | Custom path to the `byk` binary (supports `~`)        | `""`    |

## Versioning

The extension is a shell around the CLI, so its version tracks the CLI's:

| Field | Meaning |
| --- | --- |
| `version` (`X.Y.Z`) | `X.Y` equals the bundled CLI's `major.minor`; `Z` increments independently for extension-only fixes |
| `bykBinaryVersion` | The exact CLI release bundled into this build |

`0.6.3` therefore means "the fourth extension build on the byk 0.6 line", while `bykBinaryVersion` says precisely which `0.6.x` binary it ships. When the CLI moves to a new `X.Y`, the extension resets `Z` to `0`.

`scripts/download-binary.mjs` picks the release tag from `bykBinaryVersion`, so packaging is reproducible — it never resolves `latest` at build time.

To check which binary is actually running (relevant after setting `byk.binaryPath` or `byk.useSystemBinary`):

```bash
byk --version
```

## Learn more

- [byk CLI docs](https://cli.fcbyk.com)
- [Alias syntax](https://cli.fcbyk.com/cli/alias/start) — nested groups, `$cwd`, `$interactive`, placeholders, etc.
