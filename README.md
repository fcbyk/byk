# [byk](https://cli.fcbyk.com/) &middot; [![PyPI](https://img.shields.io/pypi/v/byk.svg)](https://pypi.org/project/byk/) [![Tests](https://github.com/fcbyk/byk/actions/workflows/test.yml/badge.svg)](https://github.com/fcbyk/byk/actions/workflows/test.yml) [![Coverage](https://codecov.io/gh/fcbyk/byk/branch/main/graph/badge.svg)](https://codecov.io/gh/fcbyk/byk)

> An alias runner. Commands live in `*.byk.json`, you call them by short names.

byk does two things — **find an alias, run it**. No plugins, no package manager,
no shell-profile surgery.

## Use

```json
// run.byk.json
{
  "$cwd": "~/project/web",
  "dev": "pnpm dev",
  "build": "pnpm build",
  "deploy": {
    "$cmd": "deploy --env {env}",
    "$interactive": true
  }
}
```

```bash
byk dev             # pnpm dev, run in ~/project/web
byk build           # pnpm build
byk deploy          # asks for {env}, then runs
```

`{env}` is a placeholder, `$cwd` / `$interactive` / `$description` are
inheritable metadata, `@file.key` / `@@file.key` route to one exact file.

Alias files in the current directory join the merged namespace with priority
1, global ones in `~/.byk/alias/` with 0 — so `cd` into a project and its
names just work, overriding same-named global aliases. Write
`"$priority": -1` to keep a file out of the namespace, reachable only through
`@file.key`.

## CLI

| Form | What it does |
|---|---|
| `byk <alias> [args...]` | run an alias from the merged space |
| `byk @file.key [args...]` | exact call in the current directory |
| `byk @@file.key [args...]` | exact call in `~/.byk/alias/` |
| `-h, --help` | usage, options and the full alias list |
| `-v, --version` | version, commit, platform, install path |
| `--paths` | the two directories byk uses: home and alias |
| `__api__ <request>` | read-only interface for shells and editors (hidden) |

`byk __api__ completion zsh` prints the shell registration script;
`byk __api__ complete <words...>` powers Tab completion. Neither ever writes
to disk.

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
- [Alias configuration](https://cli.fcbyk.com/cli/alias/config)
- [Placeholders](https://cli.fcbyk.com/cli/alias/placeholders)

## License

[MIT](LICENSE)
