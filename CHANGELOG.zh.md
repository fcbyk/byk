## v0.7.0 (2026-11-08)

### 破坏性变更

- **variables**: 变量语法从 `$name` 改为 `${name}`——裸写的 `$xx` 现在原样交给 shell
- **variables**: `${byk}` 更名为 `${byk_dir}`，移除 `$workdir`，新增的 `${file_dir}` 指向别名文件所在目录
- **`$mode`**: `$interactive` / `$silent` 两个布尔值改为三态 `$mode`（`normal` / `interactive` / `silent`）
- **variables**: 含空白的值不再自动加引号——需要就自己加

### 新功能

- **`$mode`**: `silent` 抑制 byk 的提示行；`interactive` 保留确认环节；可在文件间继承
- **variables**: `$var` 声明可以覆盖内置名称；`\${...}` 原样传给 shell
- **import**: `*.byk.json` 根部为单元素数组时，现在指向另一个别名文件（一层），让配置包能和脚本放在一起
- **`__api__ paths`**: 以 JSON 或纯字符串输出 CLI 目录，供 shell 脚本取用

### 问题修复

- **execution**: 提示行（来源行、解析后的命令）改走 stderr，且 stderr 不是终端时不再输出
- **`$paths`**: 相对条目改为基于配置文件所在目录解析，而非工作目录
- **variables**: 未定义的 `${NAME}` 原样传给 shell，不再被吞掉
- **variables**: 循环引用不再导致无限展开

## v0.6.0 (2026-10-29)

### 破坏性变更

- **commands**: 移除全部内置子命令（`add`、`remove`、`show`）；byk 现在是纯粹的别名执行器
- **plugins**: 移除插件协议与插件的安装、管理
- **cache**: 移除别名缓存；`*.byk.json` 的改动即时生效
- **completion**: 移除公开的 `completion` 子命令；shell 补全改用 `__api__` 接口
- **node / py**: 移除 Node.js（npm/pnpm）与 Python（py-v/uv）的初始化功能

### 新功能

- **`__api__`**: 新增隐藏内置命令 `byk __api__`，用于生成 shell 补全脚本与查询候选
- **`--paths`**: 新增 `--paths` 选项，输出 CLI 主目录与别名目录路径
- **`$var`**: 别名文件现在支持 `$var` 声明，值可按平台（linux/macos/windows）区分

## v0.5.1 (2026-07-13)

### 新功能

- **completion**: 改进上下文相关的 Tab 补全
- **plugins**: 只下载到工作目录的插件不再被持久化
- **install**: 新增流式下载与进度条

### 问题修复

- **plugins**: add 命令保存文件时自动创建父目录
- **scripts**: install.ps1 用兼容 PS 5.1 的空值判断替换 `??` 运算符

## v0.5.0 (2026-07-06)

### 破坏性变更

- **plugins**: py-module → python-m，py-script → python，py-bin → pip-bin
- **plugins**: 所有下载统一为 downloads / download-to-workdir / download-to-alias
- **plugins**: 移除 bin 与 bin-tar 字段，改用带 `[tar]` / `[exe]` 前缀的 downloads

### 新功能

- **plugins**: `pip` 与 `pip-keep` 接受单个字符串，不再只认数组
- **plugins**: `$var` 支持按平台取值
- **plugins**: 插件协议新增 `alias` 字段

### 问题修复

- **Windows**: 修复别名路径显示错误
- **Windows**: 修复别名执行错误

### 测试

- 为基础二进制的可用性新增 e2e 冒烟测试

## v0.4.0 (2026-06-27)

### 破坏性变更

- **plugins**: 从插件协议移除 `pip-e` 字段，从 add 命令移除 `-e` 选项
- **plugins**: 把 `scripts` 字段从 `commands`/`command` 中独立出来
- **add**: 用 @ 语法选取分支，取代 `-b`/`--branch` 选项
- **plugins**: 把 `install.pip` 扁平化为 `pip`，新增 `pip-keep` 用于共享依赖
- **add**: 通过 `$default` 确定性地解析插件键

### 问题修复

- 无论路径前缀如何，py-script 条目都会下载
- venv 中没有 pip（uv 模式）时 `uninstall_plugin` 失败

### 新功能

- **plugins**: 为安装与卸载新增 verbose 过程日志
- **plugins**: 新增 bin 与 bin-tar，支持按平台下载二进制
- **cli**: `-v` 输出新增编译期平台检测
- **plugins**: byk.json 插件协议支持 `$var` 变量替换
- **plugins**: 新增 `command` 字段以支持单命令插件
- **plugins**: 新增相对路径解析，移除 download 字段
- **add**: 新增 `--cdn` 选项，通过 jsDelivr 拉取 GitHub 插件
- **add**: 扩展 `--file`，支持远程 URL
- **show**: 新增 paths 子命令并简化总览
- **add**: 在 py-v 之外新增 uv 支持，用于管理 Python 环境
- **add**: 新增 py-v 功能，生成 Python venv 与 pip 别名

### 重构

- **plugins**: 扁平化 `plugins.pkg.json` 结构，改进插件列表展示

## v0.3.0 (2026-06-23)

### 破坏性变更

- 移除 init/remove py-v，`byk add` 中自动创建 venv (#7)
- 把 `init` 子命令并入 `add`
- 用 `show` 子命令取代 `--info` 选项
- 用原生 Rust 插件发现取代 bykpy，插件协议改为操作驱动的设计
- 移除全局 py 功能，只保留 py-v

### 新功能

- **remove**: 记录已安装的包，新增 `byk remove <key>` 卸载 (#7)
- **alias**: 新增 `$description` 字段，用于别名帮助展示
- **alias**: 别名文件新增 `$paths` 字段，用于前置 `PATH`

### 杂项

- 从外部仓库引入 VS Code 扩展
- 从外部仓库引入可选的 python sdk
- 新增 py-bin、py-script、py-module 示例插件

### 文档

- 新增 sdk 文档
- 新增 CLI 命令、插件系统与插件注册表文档

## v0.2.0 (2026-06-14)

### 新功能

- 新增 init/remove 子命令与零配置启动 (#4)
- 移除 `--info` 的版本检查 (#1)

### 重构

- 简化 `--info`，移除 dashboard
- 改进别名的 cwd 展示
- npm 缓存改为惰性，node-pkgs 不存在时整段跳过

### 问题修复

- bykpy 运行环境不可用时清理过期缓存
- 用 rustyline 取代 stdin 的 read_line，支持中文输入 (#2)

## v0.1.0 (2026-06-10)

首个版本。
