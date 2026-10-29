import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { defineConfig } from '@rspress/core';
import { pluginLlms } from '@rspress/plugin-llms';
import pluginFileTree from 'rspress-plugin-file-tree';

const __dirname = path.dirname(fileURLToPath(import.meta.url));

const ZH_DESCRIPTION = '一个只会执行别名的 CLI：命令写进 *.byk.json，短名字调用';
const EN_DESCRIPTION =
  'A CLI that only runs aliases: write commands into *.byk.json, call them by short names';

export default defineConfig({
  root: 'content',
  // 站点级默认值（llms.txt 标题等场景读取）；各语言可在 locales 里覆盖
  title: 'BYK',
  description: EN_DESCRIPTION,
  // 默认语言：路由不带前缀（/cli/...）；中文页面带 /zh 前缀
  lang: 'en',
  locales: [
    {
      lang: 'en',
      label: 'English',
      title: 'BYK',
      description: EN_DESCRIPTION,
    },
    {
      lang: 'zh',
      label: '简体中文',
      title: 'BYK',
      description: ZH_DESCRIPTION,
    },
  ],
  globalStyles: path.join(__dirname, 'styles', 'custom.css'),
  plugins: [
    // 按语言分组生成 llms.txt：默认语言（英文）在根目录，中文在 /zh 下
    pluginLlms([
      {
        llmsTxt: { name: 'llms.txt' },
        llmsFullTxt: { name: 'llms-full.txt' },
        include: ({ page }) => page.lang === 'en',
      },
      {
        llmsTxt: {
          name: 'zh/llms.txt',
          // 插件只读顶层 title/description，中文分组用 onTitleGenerate 覆盖标题块
          onTitleGenerate: ({ title }) => `# ${title}\n\n> ${ZH_DESCRIPTION}`,
        },
        llmsFullTxt: { name: 'zh/llms-full.txt' },
        include: ({ page }) => page.lang === 'zh',
      },
    ]),
    pluginFileTree(),
  ],
  // 自定义组件（首页 Home.tsx）的翻译文本，通过 useI18n() 读取
  i18nSource: {
    homeLede: {
      zh: '一个只会执行别名的 CLI',
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
      zh: '精确执行',
      en: 'Exact Execution',
    },
    homeFeat2DescA: {
      zh: '需要指定具体文件时用',
      en: 'When you need to target a specific file, use',
    },
    homeFeat2Token: {
      zh: '@文件.key',
      en: '@file.key',
    },
    homeFeat2DescB: {
      zh: '，绕过合并直接路由过去，同名也不会互相干扰。',
      en: 'to bypass merging and route straight to it — same-named aliases never clash.',
    },
    homeFeat2More: {
      zh: '选项与命令',
      en: 'Options & Commands',
    },
    homeShowLabel: {
      zh: '命令别名',
      en: 'Command Aliases',
    },
    homeShowTitle: {
      zh: '用 JSON 描述你的命令集合',
      en: 'Describe your command set in JSON',
    },
    homeShowSub: {
      zh: '一个文件管完项目里所有命令——占位符、交互式输入、精确执行语法都在这里。',
      en: 'One file for every command in your project — placeholders, interactive input, and exact-execution syntax, all included.',
    },
    homeShowOutput: {
      zh: '执行效果',
      en: 'Output',
    },
    homeCliLabel: {
      zh: '命令行',
      en: 'CLI Surface',
    },
    homeCliTitle: {
      zh: '三个选项，一个接口',
      en: 'Three options, one interface',
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
      zh: '选项 → 隐藏接口 →',
      en: 'Options → hidden interface →',
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
  },
  markdown: {
    link: {
      checkDeadLinks: {
        excludes: ['/llms-full.txt', '/zh/llms-full.txt'],
      },
    },
  },
  route: {
    cleanUrls: true,
  },
  themeConfig: {
    llmsUI: true,
    editLink: {
      docRepoBaseUrl: 'https://github.com/fcbyk/byk/tree/main/docs/content',
    },
    socialLinks: [
      {
        icon: 'github',
        mode: 'link',
        content: 'https://github.com/fcbyk/byk',
      },
    ],
  },
});
