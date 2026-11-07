import fs from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { defineConfig } from '@rspress/core';
import { pluginLlms } from '@rspress/plugin-llms';
import pluginFileTree from 'rspress-plugin-file-tree';
import { i18nSource } from './i18n';

const __dirname = path.dirname(fileURLToPath(import.meta.url));

// 版本的唯一真值来源是 Cargo.toml 的 [package].version（cz 也只改这里）。
// 文档里所有版本戳都由它生成，手写版本号迟早会和二进制对不上。
const CARGO_MANIFEST = path.join(__dirname, '..', 'Cargo.toml');
const VERSION = (() => {
  const matched = fs
    .readFileSync(CARGO_MANIFEST, 'utf8')
    .match(/\[package\][\s\S]*?\nversion = "([^"]+)"/);
  if (!matched) {
    throw new Error(`cannot read [package].version from ${CARGO_MANIFEST}`);
  }
  return matched[1];
})();
const VERSION_TAG = `byk v${VERSION}`;

const ZH_DESCRIPTION = '一个只会执行别名的 CLI：命令写进 *.byk.json，短名字调用';
const EN_DESCRIPTION =
  'A CLI that only runs aliases: write commands into *.byk.json, call them by short names';

/**
 * 每页生成 md 末尾的版本戳。
 *
 * 只作用于 `mdFiles`（HTML 页面不受影响），因为单个页面被 AI 直接抓取时
 * 没有 llms.txt 可依，版本只能靠这一行。语言按源文件路径判断：
 * remark 插件运行时拿不到 rspress 的 locale。
 */
function versionFooterPlugin() {
  return () => (tree: any, file: any) => {
    const isZh = String(file?.path ?? '').includes('/zh/');
    const text = (value: string) => ({ type: 'text', value });
    const code = (value: string) => ({ type: 'inlineCode', value });

    tree.children.push(
      { type: 'thematicBreak' },
      {
        type: 'paragraph',
        children: isZh
          ? [
              text(`${VERSION_TAG} · 本页可能落后于 HEAD，以 `),
              code('byk __api__'),
              text(' 与 '),
              code('byk -v'),
              text(' 为准。'),
            ]
          : [
              text(`${VERSION_TAG} · this page may lag behind HEAD; `),
              code('byk __api__'),
              text(' and '),
              code('byk -v'),
              text(' are authoritative.'),
            ],
      },
    );
  };
}

const FOR_AGENTS_EN = `
## For agents

- Fetch one page at a time: \`https://cli.fcbyk.com/<path>.md\` (\`text/markdown\`, no navigation chrome).
- Fetch everything at once: \`https://cli.fcbyk.com/llms-full.txt\`.
- These pages describe **${VERSION_TAG}**. The runtime is authoritative: \`byk -v\` for the version, \`byk __api__\` for the live interface list, \`byk -h\` for options and aliases.
- Chinese pages live under \`/zh/\`; read one language, never both.
`;

const FOR_AGENTS_ZH = `
## 给 AI 的说明

- 优先按页取：\`https://cli.fcbyk.com/zh/<path>.md\`（\`text/markdown\`，不含页面导航）。
- 一次读完：\`https://cli.fcbyk.com/zh/llms-full.txt\`。
- 这些页面描述的是 **${VERSION_TAG}**。运行时才是权威：版本看 \`byk -v\`，接口清单看 \`byk __api__\`，选项与别名看 \`byk -h\`。
- 英文页在根路径下；只读一种语言，不要两版都读。
`;

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
    // 按语言分组生成 llms.txt：默认语言（英文）在根目录，中文在 /zh 下。
    // 标题块统一带上版本号，末尾追加一段给 AI 的取用说明。
    pluginLlms([
      {
        llmsTxt: {
          name: 'llms.txt',
          onTitleGenerate: ({ title, description }) =>
            `# ${title ?? 'BYK'}\n\n> ${VERSION_TAG} — ${description ?? EN_DESCRIPTION}`,
          onAfterLlmsTxtGenerate: (content) => `${content}\n${FOR_AGENTS_EN}`,
        },
        llmsFullTxt: { name: 'llms-full.txt' },
        mdFiles: { remarkPlugins: [versionFooterPlugin()] },
        include: ({ page }) => page.lang === 'en',
      },
      {
        llmsTxt: {
          name: 'zh/llms.txt',
          // 插件只读顶层 title/description，中文分组用 onTitleGenerate 覆盖标题块
          onTitleGenerate: ({ title }) =>
            `# ${title}\n\n> ${VERSION_TAG} — ${ZH_DESCRIPTION}`,
          onAfterLlmsTxtGenerate: (content) => `${content}\n${FOR_AGENTS_ZH}`,
        },
        llmsFullTxt: { name: 'zh/llms-full.txt' },
        mdFiles: { remarkPlugins: [versionFooterPlugin()] },
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
