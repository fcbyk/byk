import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { defineConfig } from '@rspress/core';
import { pluginLlms } from '@rspress/plugin-llms';
import pluginFileTree from 'rspress-plugin-file-tree';
import { i18nSource } from './i18n';

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
  // 首页文案翻译定义在 i18n.ts（与 Home.tsx 共用，key 自动进入类型）
  i18nSource,
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
