/**
 * 首页（theme/components/Home.tsx）的翻译文本。
 *
 * 单独抽成模块是为了双向复用：
 * - rspress.config.ts 把它作为 i18nSource 传给 rspress
 * - Home.tsx 通过 useI18n<typeof i18nSource>() 获得 key 的类型提示
 *
 * 新增文案时在此处添加，key 会自动进入两边的类型。
 */
export const i18nSource = {
  homeLede: {
    zh: '一个专注于执行别名的 CLI',
    en: 'A CLI that only runs aliases',
  },
  homeSub: {
    zh: '命令写进',
    en: 'Write commands into',
  },
  homeSubTail: {
    zh: '，短名字调用',
    en: ', and call them by short names',
  },
  homeCopyAria: {
    zh: '复制安装命令',
    en: 'Copy install command',
  },
  homeViewDocs: {
    zh: '查看文档',
    en: 'View Docs',
  },
  homeFeatLabel: {
    zh: '核心',
    en: 'Core',
  },
  homeFeatTitle: {
    zh: '写 JSON，敲短命令',
    en: 'Write JSON, run short commands',
  },
  homeFeatSub: {
    zh: '一个文件说清项目里所有命令，调用时不用记完整路径和参数',
    en: 'One file holds every command in your project — no full paths or arguments to memorize',
  },
  homeFeat1Title: {
    zh: '命令别名',
    en: 'Command Aliases',
  },
  homeFeat1DescA: {
    zh: '占位符、',
    en: 'placeholders,',
  },
  homeFeat1Sep: {
    zh: '、',
    en: ' / ',
  },
  homeFeat1DescB: {
    zh: '三级继承，',
    en: 'three-level inheritance for',
  },
  homeFeat1DescC: {
    zh: '变量与平台映射。',
    en: 'variables with platform mapping.',
  },
  homeFeat1More: {
    zh: '别名配置',
    en: 'Alias Configuration',
  },
  homeFeat2Title: {
    zh: '精确路由与文件命令',
    en: 'Exact routing & file commands',
  },
  homeFeat2DescA: {
    zh: '要指定具体文件时用',
    en: 'Use',
  },
  homeFeat2Token: {
    zh: '@文件.key',
    en: '@file.key',
  },
  homeFeat2DescB: {
    zh: '绕过合并直接路由过去；要改文件本身用',
    en: 'to bypass merging and go straight to a file; to edit the file itself use',
  },
  homeFeat2Token2: {
    zh: '@文件..命令',
    en: '@file..command',
  },
  homeFeat2DescC: {
    zh: '——init、set、clone、link 这些，不必手写 JSON。',
    en: '— init, set, clone, link — no hand-written JSON.',
  },
  homeFeat2More: {
    zh: '文件命令',
    en: 'File Commands',
  },
  homeShowLabel: {
    zh: '命令别名',
    en: 'Command Aliases',
  },
  homeShowTitle: {
    zh: '全局那份，不用开编辑器',
    en: 'The global file, without an editor',
  },
  homeShowSub: {
    zh: '项目里的别名文件，手写最直接；家目录那份 ~/.byk/alias/ 用文件命令改，四条命令就写完。',
    en: 'In a project the file is easiest to write by hand; for ~/.byk/alias/, file commands finish it in four lines.',
  },
  homeShowCommands: {
    zh: '四条命令写出一个文件',
    en: 'a file in four commands',
  },
  homeCliLabel: {
    zh: '命令行',
    en: 'CLI Surface',
  },
  homeCliTitle: {
    zh: '四个选项，一组文件命令',
    en: 'Four options and a set of file commands',
  },
  homeCliSub: {
    zh: '除了别名，byk 只有这些。',
    en: 'Besides aliases, this is all byk has.',
  },
  homeTile1Label: {
    zh: '帮助',
    en: 'Help',
  },
  homeTile1Desc: {
    zh: 'banner、用法、选项和全部别名一览；不带参数等价',
    en: 'Banner, usage, options, and every alias at a glance; running bare is equivalent',
  },
  homeTile2Label: {
    zh: '版本',
    en: 'Version',
  },
  homeTile2Desc: {
    zh: '版本、commit、构建日期、平台与安装位置',
    en: 'Version, commit, build date, platform, and install location',
  },
  homeTile3Label: {
    zh: '路径',
    en: 'Paths',
  },
  homeTile3Desc: {
    zh: 'CLI 家目录与全局别名目录的实际位置',
    en: 'The actual locations of the CLI home and global alias directory',
  },
  homeTile5Label: {
    zh: '体检',
    en: 'Check',
  },
  homeTile5Desc: {
    zh: '扫一遍别名文件，按文件汇总读取、JSON、元数据与链接问题',
    en: 'Scan every alias file and report read, JSON, metadata, and link problems per file',
  },
  homeTile6Label: {
    zh: '文件命令',
    en: 'File Commands',
  },
  homeTile6Desc: {
    zh: '每个别名文件自带：init、set、remove、clone、link、json、path',
    en: 'Built into every alias file: init, set, remove, clone, link, json, path',
  },
  homeTile4Label: {
    zh: '程序接口',
    en: 'Program Interface',
  },
  homeTile4Desc: {
    zh: '只读、不写盘：给 shell 补全和编辑器插件取数据',
    en: 'Read-only and never writes to disk: feeds data to shell completion and editor plugins',
  },
  homeWhyLabel: {
    zh: '为什么选 byk',
    en: 'Why byk',
  },
  homeWhyTitle: {
    zh: '命令只收进一个入口',
    en: 'One entry point for every command',
  },
  homeWhy1Term: {
    zh: '来源可预期',
    en: 'Predictable sources',
  },
  homeWhy1BodyA: {
    zh: '选项 → 隐藏接口 → 文件命令 →',
    en: 'Options → hidden interface → file commands →',
  },
  homeWhy1BodyB: {
    zh: '→ 别名，第一个命中即执行；未命中时列出候选并退出 1。',
    en: '→ aliases — first match wins; on a miss, candidates are listed and it exits 1.',
  },
  homeWhy2Term: {
    zh: '作用域清晰',
    en: 'Clear scoping',
  },
  homeWhy2BodyA: {
    zh: '就近优先：项目文件（1）覆盖全局文件（0），进目录就能用短名字；只想给单个文件用，写',
    en: 'Near-first: project files (1) override global files (0), so short names work as soon as you cd in; to limit a file to itself, set',
  },
  homeWhy2BodyB: {
    zh: '。',
    en: '.',
  },
  homeWhy3Term: {
    zh: '跨平台',
    en: 'Cross-platform',
  },
  homeWhy3Body: {
    zh: 'Rust 编译的单个二进制，原生支持 macOS / Linux / Windows（x86_64 / arm64）。',
    en: 'A single Rust binary with native support for macOS / Linux / Windows (x86_64 / arm64).',
  },
  homeWhy4Term: {
    zh: '编辑器直连',
    en: 'Editor integration',
  },
  homeWhy4BodyA: {
    zh: '装上 byk 的 VS Code 插件，在',
    en: 'Install the byk VS Code extension to run the alias under your cursor inside',
  },
  homeWhy4BodyB: {
    zh: '里右键就能运行光标处的别名，体验与 package.json scripts 一致；也能在命令面板里检索全部别名。',
    en: 'with a right-click — same experience as package.json scripts — and search all aliases from the command palette.',
  },
  homeCloseSub: {
    zh: '几秒钟安装，把常用命令收成短名字',
    en: 'Install in seconds and turn everyday commands into short names',
  },
  homeCloseCta: {
    zh: '查看 CLI 文档',
    en: 'View CLI Docs',
  },
} satisfies Record<string, Record<string, string>>;