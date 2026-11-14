## Unreleased

### 新功能

- **variables**: 新增 `${work_dir}`，展开为调用 byk 时的目录，不受继承的 `$cwd` 影响；内置变量展开后是绝对路径，可直接写进 `$cwd` 跳出文件级目录
- **`__api__ paths`**: JSON 新增 `workdir` 与 `aliasFiles`——按文件名枚举的别名文件路径数组，先全局后本地，各自按 key 排序
- **`__api__ paths`**: `<name>` 现在也接受 `workdir`，JSON 里的每个目录字段都有对应的标量取法（`root` / `alias` / `workdir`）
- **`__api__ file`**: 新增只读请求，把单个文件 key（`@name` / `@@name` / `@` / `@@`）解析为 `{key, scope, path, dir}`；不检查存在性、不读内容、不跟随链接。加 `--list` 时以同形对象数组返回全部别名文件
- **`__api__ expand`**: 展开任意 JSON 文件中的文件级 `$var` 与内置变量并输出结果；不校验别名结构、不跟随链接、不执行任何内容

### 问题修复

- **诊断**: `--check` 现在报告非法文件名与断链符号链接；断链仍会出现在 `--paths` / `file --list`，`scanned` 改为统计遇到的候选 `*.byk.json` 目录项
- **精确执行**: 区分“文件不存在”与“文件存在但无法加载”，后者附带原因并提示 `byk --check`；路径段以 `$` 开头时按保留字段拒绝，不再把 `$cmd` / `$cwd` 等元数据当成别名执行

## v0.8.0 (2026-11-13)

### 破坏性变更

- **`$priority`**: 负数不再让文件退出合并空间，一律按 `0` 处理

### 新功能

- **`$private`**: 新增可继承字段，让文件、分组或别名不进入合并空间，精确调用照常可用；取代 `$priority: -1`
- **文件命令**: 别名文件现在自带保留命令，写作 `@文件..命令` / `@@文件..命令`——`init`、`destroy`、`clear`、`set`、`remove`、`json`、`path`、`rename`、`clone`、`link`、`help`，用来创建、编辑、克隆、链接与查看文件本身
- **completion**: 文件作用域 key（`@file` / `@@file`）只在敲下 `@` / `@@` 后出现；空 Tab 现在只列合并别名
- **completion**: 文件 key 现在在 `--with-descriptions` 下附带别名文件路径作为描述；输入 `..` 后可补全文件命令及其选项
- **`--check`**: 诊断报告；文件读取失败、JSON 错误、元数据类型错误与链接问题改为在帮助 banner 中汇总，普通执行不再直接输出
- **`--paths`**: 现在同时输出工作目录，并列出全局别名目录和当前工作目录下的别名文件；只校验文件名，不解析内容
- **`__api__ check`**: 以 JSON 输出扫描诊断，供编辑器插件使用；有问题时退出码仍为 `0`，人读的 `--check` 报告与退出码 `1` 保持不变
- **`-v`**: 可执行文件是符号链接时，安装路径后面追加目标路径
- **`$recursive`**: 新增不可继承字段，允许别名在调用链上重入自身；`BYK_ALLOW_RECURSION=1` 可临时全局放行，超过 32 层的链路则一律拦住

### 问题修复

- **execution**: 自调用别名在链路绕回时即被识别并停下，报错带完整链路（`@dev.build -> @dev.release -> @dev.build`），不再无上限地往下 fork
- **execution**: 替换进命令的值会加引号，含空格、`;`、`*` 或反斜杠的参数到命令里仍是同一个完整参数
- **completion**: 空格之后不再补全嵌套别名，只补第一个词位置

### 性能

- **execution**: `@file.key` / `@@file.key` 只解析目标别名文件，不再扫描并合并所有别名文件
- **`--check` / 补全**: 只需要文件或诊断时，扫描不再构建合并树
- **completion**: `@file.` 只读目标文件补全；文件 key 描述改为只枚举文件名，不再解析所有别名文件

## v0.7.0 (2026-11-08)

### 破坏性变更

- **variables**: 变量语法从 `$name` 改为 `${name}`——裸写的 `$xx` 现在原样交给 shell
- **variables**: `${byk}` 更名为 `${byk_dir}`，移除 `$workdir`，新增的 `${file_dir}` 指向别名文件所在目录
- **`$mode`**: `$interactive` / `$silent` 两个布尔值改为三态 `$mode`（`normal` / `interactive` / `silent`）
- **variables**: 含空白的值不再自动加引号——需要就自己加

### 新功能

- **`$mode`**: `silent` 抑制 byk 的提示行；`interactive` 保留确认环节；可在文件间继承
- **variables**: `$var` 声明可以覆盖内置名称；`\${...}` 原样传给 shell
- **link**: `*.byk.json` 根部为单元素数组时，现在指向另一个别名文件（一层），让配置包能和脚本放在一起
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
