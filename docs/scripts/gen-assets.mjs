// 构建文档站之前要准备的资源，从仓库根的唯一真值来源生成：
//
//   - changelog 页面  <- CHANGELOG*.md
//   - 首页版本号      <- Cargo.toml 的 [package].version
//
// 为什么不直接手写：更新日志每发一版就要改一次，版本号更是每次发版都变；
// 副本迟早会漏改。让根目录的文件当唯一真值来源，这些产物在 dev/build 前重新生成。
import fs from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const __dirname = path.dirname(fileURLToPath(import.meta.url));
const DOCS = path.resolve(__dirname, '..');
const REPO = path.resolve(DOCS, '..');

const PAGES = [
  {
    source: path.join(REPO, 'CHANGELOG.md'),
    output: path.join(DOCS, 'content', 'en', 'changelog.mdx'),
    title: 'Changelog',
    description:
      'Every byk release, newest first: breaking changes, features, and fixes',
    heading: 'Changelog',
    intro:
      'Newest first. Breaking changes are called out under their own heading — read them before upgrading across a minor version.',
  },
  {
    source: path.join(REPO, 'CHANGELOG.zh.md'),
    output: path.join(DOCS, 'content', 'zh', 'changelog.mdx'),
    title: '更新日志',
    description: 'byk 每个版本的变更，最新的在前：破坏性变更、新功能与问题修复',
    heading: '更新日志',
    intro:
      '最新的在前。破坏性变更单独成节——跨小版本升级前请先读它们。',
  },
];

/**
 * 转义 mdx 会当成表达式或 JSX 的字符。
 *
 * changelog 里的 `{...}` 与 `<key>` 基本都包在反引号里，那种情况不能动
 * （转义了就变成字面反斜杠）。所以按行扫描，遇到反引号切换 code span 状态。
 * 跨行的 code span 这里不处理——changelog 里没有这种写法，真出现了也只会
 * 多转义几个字符，不至于让构建失败。
 */
function escapeLine(line) {
  let out = '';
  let inCodeSpan = false;
  for (const ch of line) {
    if (ch === '`') {
      inCodeSpan = !inCodeSpan;
      out += ch;
      continue;
    }
    if (!inCodeSpan && (ch === '{' || ch === '}' || ch === '<')) {
      out += `\\${ch}`;
      continue;
    }
    out += ch;
  }
  return out;
}

function toMdx(source) {
  let inFence = false;
  return source
    .split('\n')
    .map((line) => {
      if (/^\s*```/.test(line)) {
        inFence = !inFence;
        return line;
      }
      return inFence ? line : escapeLine(line);
    })
    .join('\n');
}

for (const page of PAGES) {
  if (!fs.existsSync(page.source)) {
    throw new Error(`missing changelog source: ${page.source}`);
  }

  const body = toMdx(fs.readFileSync(page.source, 'utf8').trimEnd());
  const frontmatter = [
    '---',
    `title: ${JSON.stringify(page.title)}`,
    `description: ${JSON.stringify(page.description)}`,
    // 页面挂在根目录，没有 _meta.json，侧栏会渲染成一个空壳；changelog 也不需要它
    'sidebar: false',
    '---',
    '',
    `# ${page.heading}`,
    '',
    // 给样式表一个页面级抓手：右侧目录只保留版本号，靠 custom.css 里的
    // :has(#changelog-page) 定位，避免写到全局去影响其它文档页。
    '<div id="changelog-page"></div>',
    '',
    page.intro,
    '',
    body,
    '',
  ].join('\n');

  fs.mkdirSync(path.dirname(page.output), { recursive: true });
  fs.writeFileSync(page.output, frontmatter);
  console.log(
    `[changelog] ${path.relative(REPO, page.source)} -> ${path.relative(REPO, page.output)}`,
  );
}

/**
 * 首页 hero 的版本标签：生成一个小模块给 React 用。
 *
 * 不在运行时请求版本号——构建期就定下来，离线也能显示，也不会出现
 * “页面写着 v0.8、内容其实是 0.7 写的”这种错位。
 */
function generateVersion() {
  const manifest = path.join(REPO, 'Cargo.toml');
  const matched = fs
    .readFileSync(manifest, 'utf8')
    .match(/\[package\][\s\S]*?\nversion = "([^"]+)"/);
  if (!matched) {
    throw new Error(`cannot read [package].version from ${manifest}`);
  }

  const output = path.join(DOCS, 'theme', 'generated', 'version.ts');
  fs.mkdirSync(path.dirname(output), { recursive: true });
  fs.writeFileSync(
    output,
    [
      '// 由 docs/scripts/gen-assets.mjs 从根 Cargo.toml 生成，不要手改。',
      `export const BYK_VERSION = ${JSON.stringify(matched[1])};`,
      '',
    ].join('\n'),
  );
  console.log(
    `[version]   Cargo.toml (${matched[1]}) -> ${path.relative(REPO, output)}`,
  );
}

generateVersion();
