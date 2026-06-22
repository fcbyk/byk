# [byk](https://cli.fcbyk.com/) &middot; [![PyPI](https://img.shields.io/pypi/v/byk.svg)](https://pypi.org/project/byk/) [![Tests](https://github.com/fcbyk/byk/actions/workflows/test.yml/badge.svg)](https://github.com/fcbyk/byk/actions/workflows/test.yml) [![Coverage](https://codecov.io/gh/fcbyk/byk/branch/main/graph/badge.svg)](https://codecov.io/gh/fcbyk/byk)

> A lightweight, extensible collection of CLI utilities 🚀

Features are opt-in. Nothing is created until you need it.

## npm commands

Manage npm CLIs under a byk-scoped environment — no global pollution.

```bash
byk add npm           # first use only, activates & creates ni/nu aliases
byk ni live-server     # install (ni = npm i)
byk live-server        # run it
```

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
curl -fsSL https://raw.githubusercontent.com/fcbyk/byk/main/scripts/install.sh | bash
```

## License

[MIT](LICENSE)