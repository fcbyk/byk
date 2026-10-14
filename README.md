# [byk](https://cli.fcbyk.com/) &middot; [![PyPI](https://img.shields.io/pypi/v/byk.svg)](https://pypi.org/project/byk/) [![Tests](https://github.com/fcbyk/byk/actions/workflows/test.yml/badge.svg)](https://github.com/fcbyk/byk/actions/workflows/test.yml) [![Coverage](https://codecov.io/gh/fcbyk/byk/branch/main/graph/badge.svg)](https://codecov.io/gh/fcbyk/byk)

> A lightweight, extensible collection of CLI utilities 🚀

Two sources of commands: plugins you install, aliases you write.

## Plugins

Install commands from any GitHub repo. Python plugins run in a shared venv,
binary plugins need nothing else.

```bash
byk add user/repo            # install default plugin
byk add user/repo/key        # install a specific plugin
byk add user/repo@v1.0/key   # pin a branch, tag or commit
byk <command>                # run it

byk show plugins             # list installed plugins
byk remove <key>             # uninstall
```

See [plugin registry](https://cli.fcbyk.com/cli/plugin-registry) for details.

## Aliases

Give long commands short names.

```JSON
// ~/.byk/alias/global.byk.json
{
  "ssh": {
    "prod": "ssh -i ~/.ssh/prod.pem ubuntu@203.0.113.42"
  }
}
```

```bash
byk ssh.prod
```

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

## License

[MIT](LICENSE)