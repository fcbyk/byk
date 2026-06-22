# BYK

VS Code integration for [byk](https://github.com/fcbyk/byk) — run aliases, validate configs, and get path completion in `*.byk.json` files.

## Features

- **Right-click to run** — execute aliases directly from the editor or Explorer
- **Quick Pick** — search and run any alias via the command palette
- **JSON validation** — auto-validates `*.byk.json` and `byk.json` schemas
- **Path completion** — auto-complete file paths in `$cwd` and `$paths` fields
- **Task provider** — register `byk` as a VS Code task type
- **Bundled binary** — ships with platform-specific binaries, no extra install needed

## Usage

**Step 1** — Create a `*.byk.json` file in your project:

```jsonc
{
  "dev": "vite",
  "build": "vite build"
}
```

**Step 2** — Open the file in the editor, **right-click any line → Run Alias** to execute the alias at the cursor.

Alternatively, **right-click → Alias List** to search and run any alias from the file via the Quick Pick panel.

You can also right-click a `*.byk.json` file in the Explorer and select **Alias List**.

### Supported Filenames

| File               | Execution               |
| ------------------ | ----------------------- |
| `run.byk.json`     | `byk @run.dev`          |
| `release.byk.json` | `byk @release.publish`  |
| `.byk.json`        | `byk @.build`           |

The extension auto-activates when the project contains `*.byk.json`.

## Settings

Configure via VS Code `settings.json`:

| Setting               | Description                                             | Default |
| --------------------- | ------------------------------------------------------- | ------- |
| `byk.useSystemBinary` | Use the system PATH `byk` instead of the bundled binary | `false` |
| `byk.binaryPath`      | Custom path to the `byk` binary (supports `~`)          | `""`    |

## Learn More

- [byk CLI docs](https://cli.fcbyk.com)
- [Alias syntax](https://cli.fcbyk.com/cli/alias/start) — nested groups, `$cwd`, `$interactive`, placeholders, etc.
- [Plugin system](https://cli.fcbyk.com/cli/plugins)