# BYK

VS Code integration for [byk](https://github.com/fcbyk/byk) — run aliases from `*.byk.json` files without leaving the editor.

## Features

- **Right-click to run** — execute the alias under the cursor, from the editor or the Explorer
- **Quick Pick** — search every alias in a file via the command palette
- **JSON validation** — validates `*.byk.json` against the alias schema
- **Path completion** — completes file paths in `$cwd` values and `$paths` entries
- **Task provider** — registers `byk` as a VS Code task type, so output lands in the task panel
- **Bundled binary** — ships with platform binaries and uses them by default; two settings reorder the lookup when you need a system or development build instead

## Usage

**Step 1** — Create a `*.byk.json` file in your project:

```jsonc
{
  "dev": "vite",
  "build": "vite build"
}
```

**Step 2** — Open the file and **right-click anywhere → Run Alias** to run the alias at the cursor.

Alternatively, **right-click → Alias List** to search all aliases in the file through the Quick Pick panel. You can also right-click a `*.byk.json` file in the Explorer and pick **Alias List**.

The extension activates automatically when the workspace contains any `*.byk.json`.

### Supported filenames

| File               | Runs as                |
| ------------------ | ---------------------- |
| `run.byk.json`     | `byk @run.dev`         |
| `release.byk.json` | `byk @release.publish` |
| `.byk.json`        | `byk @.build`          |

The `@file.key` form calls the alias in that exact file, so a project file works even though it does not take part in global alias merging.

## Settings

### Choosing which `byk` runs

Two settings, one mechanism: both feed the `PATH` handed to every task, and the
first directory containing `byk` / `byk.exe` wins.

| Setting | Type | Default | Meaning |
| --- | --- | --- | --- |
| `byk.priorityPaths` | `string[]` | `[]` | **First priority.** Directories searched in array order, ahead of everything else. Absolute paths only, `~` is expanded, missing directories are skipped silently |
| `byk.secondarySource` | `bundled` \| `system` | `bundled` | **Second priority.** Which tier comes next when the directories above miss — the other one becomes the final fallback |

The resulting lookup order:

| `priorityPaths` | `secondarySource` | Order |
| --- | --- | --- |
| entries | `bundled` | `priorityPaths` → bundled → `PATH` |
| entries | `system` | `priorityPaths` → `PATH` → bundled |
| empty | `bundled` | bundled → `PATH` |
| empty | `system` | `PATH` → bundled |

`system` means your `PATH`, nothing more — the extension adds no install
locations of its own. To use `~/.byk/bin`, list it in `priorityPaths`:

```jsonc
{
  "byk.priorityPaths": ["~/dev/byk/target/release", "~/.byk/bin"],
  "byk.secondarySource": "bundled"
}
```

Note that the binary has to be named `byk` (`byk.exe` on Windows) — these are
directories on a search path, not pointers to individual files.

### Which binary am I actually running?

Run **BYK: Show Active byk** from the command palette. It prints the resolved
search order, marks the directory that won, and reports that binary's
`--version`. Prefer it over running `byk --version` in a terminal — the
terminal's `PATH` is not the one the extension hands to its tasks.

## Versioning

The extension is a shell around the CLI, so its version tracks the CLI's:

| Field | Meaning |
| --- | --- |
| `version` (`X.Y.Z`) | `X.Y` equals the bundled CLI's `major.minor`; `Z` increments independently for extension-only fixes |
| `bykBinaryVersion` | The exact CLI release bundled into this build |

`0.6.3` therefore means "the fourth extension build on the byk 0.6 line", while `bykBinaryVersion` says precisely which `0.6.x` binary it ships. When the CLI moves to a new `X.Y`, the extension resets `Z` to `0`.

`scripts/download-binary.mjs` picks the release tag from `bykBinaryVersion`, so packaging is reproducible — it never resolves `latest` at build time.

Packaging also cross-checks the repository's `Cargo.toml`, which is the source of truth for CLI releases: bundling a CLI **newer** than the one declared there fails the build (that tag cannot exist yet), while lagging **behind** only prints a hint naming the newer release.

## Learn more

- [byk CLI docs](https://cli.fcbyk.com)
- [Alias syntax](https://cli.fcbyk.com/cli/alias/start) — nested groups, `$cwd`, `$mode`, placeholders, etc.

---

*The rest of this page is the Simplified Chinese version of this README — if you have finished the English section above, you can stop here.*

# BYK（简体中文）

VS Code 的 [byk](https://github.com/fcbyk/byk) 集成——不用离开编辑器就能跑 `*.byk.json` 里的别名。

## 功能

- **右键运行** —— 在编辑器或资源管理器里执行光标所在的别名
- **快速选择** —— 通过命令面板搜索文件里的每一个别名
- **JSON 校验** —— 按别名 schema 校验 `*.byk.json`
- **路径补全** —— 为 `$cwd` 的值和 `$paths` 的条目补全文件路径
- **任务提供器** —— 把 `byk` 注册成 VS Code 的任务类型，输出落在任务面板里
- **内置二进制** —— 随包分发各平台的二进制并默认使用；需要换成系统版本或自编译版本时，两个设置可以改变查找顺序

## 使用

**第一步** —— 在项目里新建一个 `*.byk.json`：

```jsonc
{
  "dev": "vite",
  "build": "vite build"
}
```

**第二步** —— 打开这个文件，**在任意位置右键 → 运行别名**，光标所在的别名就会执行。

也可以**右键 → 别名列表**，用快速选择面板搜索该文件里的全部别名；在资源管理器里右键一个 `*.byk.json`，同样能选**别名列表**。

工作区里存在任何 `*.byk.json` 时，插件会自动激活。

### 支持的文件名

| 文件               | 实际执行               |
| ------------------ | ---------------------- |
| `run.byk.json`     | `byk @run.dev`         |
| `release.byk.json` | `byk @release.publish` |
| `.byk.json`        | `byk @.build`          |

`@文件.key` 形式只调用该文件里的别名，所以项目内的文件即使不参与全局别名合并也能用。

## 设置

### 选择用哪个 `byk`

两个设置，一套机制：都作用于交给每个任务的 `PATH`，第一个含 `byk` / `byk.exe` 的目录胜出。

| 设置 | 类型 | 默认值 | 含义 |
| --- | --- | --- | --- |
| `byk.priorityPaths` | `string[]` | `[]` | **第一优先。** 按数组顺序在这些目录里查找，排在所有来源之前。必须写绝对路径，支持 `~` 展开，不存在的目录静默跳过 |
| `byk.secondarySource` | `bundled` \| `system` | `bundled` | **第二优先。** 上面的目录都没命中时，接下来用哪一档——另一档成为最后的兜底 |

最终的查找顺序：

| `priorityPaths` | `secondarySource` | 顺序 |
| --- | --- | --- |
| 有内容 | `bundled` | `priorityPaths` → 内置 → `PATH` |
| 有内容 | `system` | `priorityPaths` → `PATH` → 内置 |
| 为空 | `bundled` | 内置 → `PATH` |
| 为空 | `system` | `PATH` → 内置 |

`system` 就是你的 `PATH`，仅此而已——插件不会自己追加任何安装位置。想用 `~/.byk/bin`，就把它写进 `priorityPaths`：

```jsonc
{
  "byk.priorityPaths": ["~/dev/byk/target/release", "~/.byk/bin"],
  "byk.secondarySource": "bundled"
}
```

注意二进制必须叫 `byk`（Windows 上是 `byk.exe`）——这些是搜索路径上的目录，不是指向具体文件的指针。

### 我实际在跑哪个二进制？

从命令面板运行 **BYK: 查看当前使用的 byk**。它会打印解析后的查找顺序、标出命中的那个目录，并报告该二进制的 `--version`。别用终端里的 `byk --version` 代替——终端的 `PATH` 不是插件交给任务的 `PATH`。

## 版本

插件是 CLI 外面的一层壳，所以版本跟着 CLI 走：

| 字段 | 含义 |
| --- | --- |
| `version`（`X.Y.Z`） | `X.Y` 等于内置 CLI 的 `major.minor`；`Z` 为纯插件侧的修复独立递增 |
| `bykBinaryVersion` | 本次构建实际内置的 CLI 版本 |

所以 `0.6.3` 的含义是「byk 0.6 线上的第四个插件构建」，究竟内置哪个 `0.6.x` 由 `bykBinaryVersion` 说清。CLI 进入新的 `X.Y` 时，插件的 `Z` 归零。

`scripts/download-binary.mjs` 从 `bykBinaryVersion` 取 release tag，所以打包是可复现的——它从不在构建期解析 `latest`。

打包时还会与仓库的 `Cargo.toml`（CLI 版本的真值来源）交叉校验：内置的 CLI 比它声明的**新**会让构建失败（那个 tag 还不存在），比它**旧**只会打印一行提示，点名更新的版本。

## 了解更多

- [byk CLI 文档](https://cli.fcbyk.com/zh)
- [别名语法](https://cli.fcbyk.com/zh/cli/alias/start) —— 嵌套分组、`$cwd`、`$mode`、占位符等
