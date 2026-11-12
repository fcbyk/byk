# [byk](https://cli.fcbyk.com/zh) &middot; [![PyPI](https://img.shields.io/pypi/v/byk.svg)](https://pypi.org/project/byk/) [![CI](https://github.com/fcbyk/byk/actions/workflows/ci.yml/badge.svg)](https://github.com/fcbyk/byk/actions/workflows/ci.yml) [![Coverage](https://codecov.io/gh/fcbyk/byk/branch/main/graph/badge.svg)](https://codecov.io/gh/fcbyk/byk)

[English](README.md) &middot; **简体中文**

> 一个别名执行器。命令写进 `*.byk.json`，用短名字调用它们。

byk 只做两件事——**找到别名，执行命令**。一个二进制，一套配置格式。

## 使用

```json
// run.byk.json
{
  "$cwd": "~/project/web",
  "dev": "pnpm dev",
  "build": "pnpm build",
  "deploy": {
    "$cmd": "deploy --env {env}",
    "$mode": "interactive"
  }
}
```

```bash
byk dev             # pnpm dev，在 ~/project/web 下执行
byk build           # pnpm build
byk deploy          # 先问 {env}，然后执行
```

`{env}` 是占位符，`$cwd` / `$mode` / `$private` 是可继承的元数据（`$mode` 取 `normal` / `interactive` / `silent`），`$description` 提供帮助文本，`@文件.key` / `@@文件.key` 精确路由到某一个文件。

当前目录里的别名文件以优先级 1 加入合并空间，全局的（`~/.byk/alias/`）以优先级 0——所以 `cd` 进一个项目，它的名字就能直接用，并覆盖同名的全局别名。给文件、分组或别名标上 `"$private": true`，它就不会进入合并空间，只保留通过 `@文件.key` 调用的能力。

## 命令行

| 形式 | 作用 |
|---|---|
| `byk <别名> [参数...]` | 在合并空间里查找并执行别名 |
| `byk @file.key [参数...]` | 精确执行**当前目录**某个文件里的别名 |
| `byk @@file.key [参数...]` | 精确执行 `~/.byk/alias/` 里某个文件的别名 |
| `-h, --help` | 用法、选项与完整别名列表 |
| `-v, --version` | 版本、commit、平台、安装路径 |
| `--paths` | CLI 家目录、别名目录、工作目录，以及全局与当前目录下的别名文件 |
| `--check` | 检查别名文件，按文件汇总扫描发现的问题 |
| `__api__ <request>` | 给 shell 和编辑器用的只读接口（隐藏） |

`byk __api__ completion zsh` 打印 shell 注册脚本；`byk __api__ complete <words...>` 为 Tab 补全提供候选。

## 安装

```bash
pip install byk
```

或用安装脚本（不需要 Python）：

```bash
curl -fsSL https://cli.fcbyk.com/install.sh | bash
```

Windows：

```powershell
powershell -c "irm https://cli.fcbyk.com/install.ps1 | iex"
```

## 文档

- [概述](https://cli.fcbyk.com/zh/cli)
- [选项与命令](https://cli.fcbyk.com/zh/cli/commands)
- [别名配置](https://cli.fcbyk.com/zh/cli/alias/config)
- [参数处理](https://cli.fcbyk.com/zh/cli/alias/placeholders)

## 许可证

[MIT](LICENSE)
