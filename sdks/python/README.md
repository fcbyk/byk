# byksdk

Python SDK for building byk plugins — zero dependencies at the top level.

## Features

- **Zero dependencies** — all top-level imports use only the Python standard library. Third-party dependencies are guarded by `@requires` decorators and loaded on demand.
- **Plugin context** — `plugin()` gives you persistence, logging, and directory management in one call.
- **Built-in tools** — network utilities, CLI output helpers, and web application assistants.

## Installation

```bash
pip install byksdk
```

## Quick Start

```python
from byksdk import plugin

ctx = plugin("my_plugin")
ctx.logger.info("plugin started")
ctx.state().set("initialized", True)
```

## Directory Layout

```
~/.byk/
├── plugins/                 # plugin sandbox
│   └── my_plugin/
│       ├── state.json       # ctx.state()
│       └── my_plugin.log    # ctx.logger
├── state/                   # global persistence
│   └── plugins.state.json   # ctx.app.store()
├── logs/
│   └── plugins.log          # ctx.app.logger
├── runtime/
└── cache/
```

## Core APIs

### Plugin Context

```python
from byksdk import plugin

ctx = plugin("server")

# plugin-scoped persistence
ctx.state().set("port", 8080)
ctx.state("config").set("timeout", 30)

# plugin-scoped logging
ctx.logger.info("server started")

# global persistence (shared across plugins)
ctx.app.store().set("theme", "dark")

# global logging
ctx.app.logger.info("app started")
```

### StateStore — JSON-based persistence

```python
store = ctx.state()

store.set("key", "value")
store.get("key")              # "value"
store.get("missing", default) # default
store.update({"a": 1, "b": 2})
store.delete("key")
store.clear()
store.load()                  # returns the full dict
```

### Network

```python
from byksdk import get_private_networks

networks = get_private_networks()
# [{ "iface": "eth0", "ips": ["192.168.1.100"], "type": "ethernet", ... }]
```

### CLI Helpers

```python
from byksdk import check_port, colored_key_value

if not check_port(8080):
    return

click.echo(colored_key_value("URL", "http://localhost:8080"))
```

### Web — SPA + unified response

```python
from byksdk import create_spa, R

app = create_spa("dist")

@app.route("/api/hello")
def hello():
    return R.ok({"message": "Hello"})
```

## Dependency Model

byksdk follows "who uses it declares it":

- Top-level modules use only the Python standard library.
- Functions requiring third-party packages use `@requires` decorators that raise a clear `ImportError` when the dependency is missing.

| If you use... | declare in `pyproject.toml` |
|---|---|
| `create_spa` / `R` / `get_client_ip` | `flask>=2.0` |
| `copy_to_clipboard` | `pyperclip>=1.9.0` |
| `get_private_networks` | `psutil>=5.9.0` |
| `check_port` / `colored_key_value` | `click>=8.0.0` |
| all other APIs | nothing extra needed |

## License

MIT