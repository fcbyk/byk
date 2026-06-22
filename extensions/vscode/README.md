# Byk Alias Runner

Right-click to run [byk](https://github.com/fcbyk/byk) aliases in VS Code.

## Usage

**Step 1** — Create a `*.byk.json` file in your project:

```jsonc
{
  "dev": "vite",       // right-click → Run Alias (Exact)
  "build": "vite build"   // executes: byk @run.build
}
```

**Step 2** — Open the file in the editor, **right-click any line → Run Alias (Exact)** to execute the alias at the cursor.

Alternatively, **right-click → Choose Alias…** to search and run any alias from the file via the Quick Pick panel.

You can also right-click a `*.byk.json` file in the Explorer and select **Choose Alias…**.

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

## Alias Syntax

For detailed alias configuration (nested groups, `$cwd`, `$interactive`, placeholders, etc.), see the [byk alias docs](https://cli.fcbyk.com/alias/start).
