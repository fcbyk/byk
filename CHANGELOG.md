## v0.3.0 (2026-06-23)

## Breaking Changes

- remove init/remove py-v, auto-create venv in `byk add` (#7)
- merge `init` subcommand into `add`
- replace --info option with `show` subcommand
- replace bykpy with native Rust plugin discovery，plugin protocol is operation-driven design 
- remove global py feature, keep only py-v

### Features

- **remove**: track installed packages and add `byk remove <key>` uninstall (#7)
- **alias**: add $description field for alias help display
- **alias**: add $paths field to alias files for PATH prepending

### Chores

- add VS Code extension from external repository
- add optional python sdk from external repository
- add py-bin、py-script、py-module example plugins

### Documentation

- add sdk documentation
- add CLI commands, plugin system, and plugin registry docs

## v0.2.0 (2026-06-14)

### Features

- add init/remove subcommands and zero-config startup (#4)
- remove --info latest version check (#1)

### Refactor

- simplify --info, remove dashboard
- improve alias cwd display
- make npm cache lazy, skip entire path when node-pkgs absent

### Bug Fixes

- clear stale cache when bykpy runtime is unavailable
- replace stdin read_line with rustyline for CJK-aware input (#2)

## v0.1.0 (2026-06-10)

Initial release.
