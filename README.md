# [byk](https://cli.fcbyk.com/) &middot; [![PyPI](https://img.shields.io/pypi/v/byk.svg)](https://pypi.org/project/byk/) [![CI](https://github.com/fcbyk/byk/actions/workflows/ci.yml/badge.svg)](https://github.com/fcbyk/byk/actions/workflows/ci.yml) [![Coverage](https://codecov.io/gh/fcbyk/byk/branch/main/graph/badge.svg)](https://codecov.io/gh/fcbyk/byk)

**English** &middot; [简体中文](README.zh.md)

> An alias runner. Commands live in `*.byk.json`, you call them by short names.

byk does two things — **find an alias, run it**. One binary, one config format.

## Use

byk scans every `*.byk.json` in the **current directory** and in the global `~/.byk/alias/` — each key in those files is a command you can run.

Write a `run.byk.json` in your project by hand:

```json
// run.byk.json
{
  "dev": "pnpm dev",
  "build": "pnpm build",
  "deploy": {
    "$cmd": "deploy --env {env}",
    "$mode": "interactive"
  }
}
```

```bash
byk dev             # pnpm dev
byk build           # pnpm build
byk deploy          # asks for {env}, confirms, then runs
```

`{env}` is a placeholder — `byk deploy` asks what to put there, then splices it in; `$mode: "interactive"` means it confirms before running; commands run in the directory you invoke byk from. For the other fields see [Alias Configuration](https://cli.fcbyk.com/cli/alias/config).

To use it from any directory, put the file in `~/.byk/alias/`. Opening an editor there is awkward, so let byk edit it (`@@` targets the global directory):

```bash
byk @@ops..init                          # create ~/.byk/alias/ops.byk.json
byk @@ops..set clean --cmd "cargo clean" # add one entry
byk @@ops..json                          # look at what is there now
```

Already have one in a project? `byk @@run..clone @run` copies it over.

## CLI

Four invocation forms:

- `byk <alias> [args...]` — look it up across the current and global directories, then run it
- `byk @file.key` / `@@file.key` — route straight to one exact file
- `byk @file..command` — file commands: `init`, `set`, `remove`, `json`, `path`, `rename`, `clone`, `link`, `destroy`, `clear`, `help`; `byk @file..help` lists them all
- `byk __api__` — read-only interface for shell completion and editor plugins (hidden)

Options: `--paths` lists the alias files byk scans, `--check` validates them, `-h` shows help and every alias, `-v` shows the version. Full descriptions live in [Options & Commands](https://cli.fcbyk.com/cli/commands) and [File Commands](https://cli.fcbyk.com/cli/file-commands).

## Installation

```bash
pip install byk
```

Or via shell script (no Python needed):

```bash
curl -fsSL https://cli.fcbyk.com/install.sh | bash
```

Windows:

```powershell
powershell -c "irm https://cli.fcbyk.com/install.ps1 | iex"
```

## Docs

- [Overview](https://cli.fcbyk.com/cli)
- [Options and commands](https://cli.fcbyk.com/cli/commands)
- [File commands](https://cli.fcbyk.com/cli/file-commands)
- [Alias configuration](https://cli.fcbyk.com/cli/alias/config)
- [Placeholders](https://cli.fcbyk.com/cli/alias/placeholders)

## License

[MIT](LICENSE)
