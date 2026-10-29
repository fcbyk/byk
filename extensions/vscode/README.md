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

## Learn more

- [byk CLI docs](https://cli.fcbyk.com)
- [Alias syntax](https://cli.fcbyk.com/cli/alias/start) — nested groups, `$cwd`, `$interactive`, placeholders, etc.
