# AGENTS.md

这个仓库里改代码、改文档时的约定。

**只写 CI 拦不住的规矩。** 代码格式、lint、测试、模块依赖这些，CI 每次提交都会自动跑
（见 `.github/workflows/ci.yml`），在这里再写成一条只是重复。

CI 管不了的，是下面五节：改不改代码、README 怎么写、CHANGELOG 怎么写、代码注释怎么写、
提交信息怎么写。

## 改不改

用户没明确说要落地改代码的，不动代码。问问题、讨论设计这类场景只做分析和回答——
问了一个问题，不等于让你改跟这个问题相关的代码。

## README

README 是给第一次来的人看的：这是什么、怎么装、怎么跑一次。**不是** docs 的目录。

- 每个命令域只留一行：形式 + 一句它管什么。动词清单、参数、行为细节都指向 docs，
  `-h` 已经能列出来的东西不在 README 里再抄一份；`@file` 那行的动词清单例外——它就是
  别名文件能做什么；
- `@@@` 只说它指向「别名文件以外的其他文件」，最多点出 `@@@config` / `@@@tap` /
  `@@@shell` 三个名字，不展开各自的动词；
- 示例只留能直接复制运行的最小串，不写预期输出、不写注意事项；
- 顶部有一条给 AI agent 看的 HTML 注释，指向本文件；渲染时不可见，别删；
- `README.md` 与 `README.zh.md` 必须同步改，结构、条目数、链接一一对应。

## CHANGELOG

CHANGELOG 只回答「改了什么」，一段话说清就够；**为什么这么改、怎么实现**是 docs 的事。

- 一条一个功能，一行到两行。不写实现细节、不写取舍论证、不列测试数字；
- 同一功能不拆成多条。例如 `@@@config` / `@@@tap` / `@@@shell` / `banner` / `@@@` 补全
  是同一件事，合成一条 **`@@@` 系统文件**；`$deps` 的代理与进度条也合成一条；
- 细节去 `docs/content/{en,zh}/cli/` 展开，CHANGELOG 里不重复。`@@@shell` 的探测顺序、
  写入纪律这类内容只出现在 `system-files.mdx`；
- 破坏性变更写清「旧写法 → 新写法」，并带上必要的取值：合并条目后，读者可能没有别处
  可查（如 `net-github` 的 `direct` / `ghproxy` 就写在 `clone` 那条里）；
- 结构照旧：`## Unreleased` + `### Breaking Changes` / `### Features` / `### Bug Fixes`；
  中文版是 `## 未发布` + `### 破坏性变更` / `### 新功能` / `### 问题修复`；版本标题写作
  `## v0.8.2 (2026-11-20)`；
- 条目格式 `- **范围**: 说明`：范围加粗，说明里出现的代码用反引号包住；
- `CHANGELOG.md` 与 `CHANGELOG.zh.md` 必须同步改：条目顺序、数量、段落标题一一对应。
  `docs/content/{en,zh}/changelog.mdx` 由脚本生成，**不要手改**。

## 注释

注释只解释「为什么」。代码自己说得清的「做了什么」，不写。

- 一行能说完就一行。超过两行，说明这段代码该拆，或者那段解释该搬到 docs；
- 不写分点清单、不写背景漫谈、不重复类型名和函数名已经表达的信息；
- 模块头 `//!` 只写它负责什么，最多再加一条最容易踩的坑；设计取舍留给 docs；
- 真需要长解释的，写进 `docs/`，注释里给一句话和位置，不在代码里铺开。

## 提交

默认**只有一行**，没有 body：

```
feat(filecmd): add @@@shell for shell completion
```

- 英文、`type(scope): subject`，subject 小写开头、不加句号、不加 emoji；代码里的东西用
  反引号包住（`@@@tap update`、`--paths`）；
- 类型照 `pyproject.toml` 里 commitizen 配的那几个：`feat` / `fix` / `refactor` / `docs` /
  `perf` / `test` / `style` / `chore`；scope 用模块名（`filecmd` / `completion` / `aliases` /
  `api` / `docs`）；
- 破坏性变更在类型后加 `!`（`feat(filecmd)!: ...`），并在 body 里写一行
  `BREAKING CHANGE: <说明>`，说明**旧写法该改成什么**（写成「旧写法 → 新写法」）：

  ```
  feat(filecmd)!: move file verbs to space separated form

  BREAKING CHANGE: `@file.set` -> `@file set`, `@@file.set` -> `@@file set`
  ```

  标题只说改了哪儿，迁移办法放这行——`!` 是标记，`BREAKING CHANGE:` 才是给读者的内容。
  `!` 不在 commitizen 的解析正则里，但 CHANGELOG 是手写的，不影响；
- 以下两种情况才写 body：**一行概括不完**，或者**一次提交里混了几件不相关的事**。
  破坏性变更属于前一种——没有 `BREAKING CHANGE:` 那一行，光看标题别人不知道怎么改。

body 同样简短：每条一行，说清改了哪里、为什么，不铺开说明。**docs 里已经写过的细节不要再
抄进 body**——`@@@shell` 的探测顺序、`CHANGELOG` 的取舍理由都在文档里，body 只补 commit
本身需要交代的东西。

## 自检

改完 README、CHANGELOG、注释，或写完 commit body，问自己两句：这段细节是不是 docs 里
已经有了？这一条能不能再删一半字？答案是「有」和「能」，就删。
