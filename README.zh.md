# [byk](https://cli.fcbyk.com/zh) &middot; [![PyPI](https://img.shields.io/pypi/v/byk.svg)](https://pypi.org/project/byk/) [![CI](https://github.com/fcbyk/byk/actions/workflows/ci.yml/badge.svg)](https://github.com/fcbyk/byk/actions/workflows/ci.yml) [![Coverage](https://codecov.io/gh/fcbyk/byk/branch/main/graph/badge.svg)](https://codecov.io/gh/fcbyk/byk)

[English](README.md) &middot; **简体中文**

> 一个别名执行器。命令写进 `*.byk.json`，用短名字调用它们。

byk 只做两件事——**找到别名，执行命令**。一个二进制，一套配置格式。

## 使用

byk 扫描**当前目录**和全局目录 `~/.byk/alias/` 下的所有 `*.byk.json`，文件里的 key 都能直接当命令敲。

项目里手写一个 `run.byk.json`：

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
byk deploy          # 先问 {env}，确认后执行
```

`{env}` 是占位符——`byk deploy` 会先问你要填什么，再拼进命令；`$mode: "interactive"` 表示先确认再执行；命令在你运行 byk 的目录里执行。其余字段见 [别名配置](https://cli.fcbyk.com/zh/cli/alias/config)。

想在任何目录都能用，把文件放进 `~/.byk/alias/`。那里不方便随手开编辑器，就让 byk 自己改（`@@` 指全局目录）：

```bash
byk @@ops..init                          # 建 ~/.byk/alias/ops.byk.json
byk @@ops..set clean --cmd "cargo clean" # 加一条
byk @@ops..json                          # 看一眼现在写了什么
```

项目里已经有现成的，`byk @@run..clone @run` 可以直接复制过去。

## 命令行

调用形式只有四种：

- `byk <别名> [参数...]` —— 在当前目录与全局目录的别名里查找并执行
- `byk @文件.key` / `@@文件.key` —— 精确路由到某一个文件
- `byk @文件..命令` —— 文件命令，`init`、`set`、`remove`、`json`、`path`、`rename`、`clone`、`link`、`destroy`、`clear`、`help`；`byk @文件..help` 一次列全
- `byk __api__` —— 给 shell 补全与编辑器插件的只读接口（隐藏）

选项：`--paths` 列出 byk 扫描到的别名文件，`--check` 检查这些文件写得对不对，`-h` 看帮助与全部别名，`-v` 看版本。完整说明见 [选项与命令](https://cli.fcbyk.com/zh/cli/commands) 和 [文件命令](https://cli.fcbyk.com/zh/cli/file-commands)。

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
- [文件命令](https://cli.fcbyk.com/zh/cli/file-commands)
- [别名配置](https://cli.fcbyk.com/zh/cli/alias/config)
- [参数处理](https://cli.fcbyk.com/zh/cli/alias/placeholders)

## 许可证

[MIT](LICENSE)
