import { useState } from 'react';
import { Link } from '@rspress/core/theme';
import './home.css';

/**
 * byk docs — 自定义首页
 *
 * 设计方向：terminal-native。
 * byk 是终端工具，哲学是 "opt-in, nothing until you need it"。
 * 所以页面克制——等宽字体扛个性，颜色只活在终端里，
 * 其余是 ink-on-paper + 发丝线。
 *
 * signature：Hero 的终端块，带真实 ANSI 风格语法着色与闪烁光标。
 */
type InstallTab = 'pip' | 'curl' | 'ps';

const INSTALLS: Record<InstallTab, { label: string; cmd: string }> = {
  pip: {
    label: 'pip',
    cmd: 'pip install byk',
  },
  curl: {
    label: 'macOS & Linux',
    cmd: 'curl -fsSL https://cli.fcbyk.com/install.sh | bash',
  },
  ps: {
    label: 'Windows',
    cmd: 'powershell -c "irm https://cli.fcbyk.com/install.ps1 | iex"',
  },
};

export function BykHome() {
  const [activeTab, setActiveTab] = useState<InstallTab>('pip');
  const [copied, setCopied] = useState(false);

  const copyInstall = () => {
    navigator.clipboard?.writeText(INSTALLS[activeTab].cmd).then(() => {
      setCopied(true);
      setTimeout(() => setCopied(false), 1800);
    });
  };

  return (
    <main className="byk-home">
      {/* ============ Hero ============ */}
      <header className="b-shell b-hero b-reveal">
        <div className="b-hero__eyebrow">
          <span className="b-dot" />
          <span>v0.5.0</span>
          <span style={{ opacity: 0.4 }}>·</span>
          <span>MIT</span>
          <span style={{ opacity: 0.4 }}>·</span>
          <span>Rust</span>
        </div>

        <h1 className="b-hero__wordmark">
          byk<span className="b-caret" aria-hidden="true" />
        </h1>

        <p className="b-hero__lede">轻量级、可扩展的命令行工具集</p>
        <p className="b-hero__sub">
          Features are opt-in. Nothing is created until you need it.
        </p>

        <div className="b-install-row">
          <div className="b-install-tabs">
            {(Object.keys(INSTALLS) as InstallTab[]).map((k) => (
              <button
                key={k}
                type="button"
                className={
                  'b-install-tab' +
                  (k === activeTab ? ' b-install-tab--active' : '')
                }
                onClick={() => { setActiveTab(k); setCopied(false); }}
                aria-pressed={k === activeTab}
              >
                {INSTALLS[k].label}
              </button>
            ))}
          </div>
          <div className="b-copy">
            <span className="b-copy__cmd">
              <span className="b-prompt">$</span>
              {INSTALLS[activeTab].cmd}
            </span>
            <button
              type="button"
              className={
                'b-copy__btn' + (copied ? ' b-copy__btn--done' : '')
              }
              onClick={copyInstall}
              aria-label="复制安装命令"
            >
              {copied ? 'copied ✓' : 'copy'}
            </button>
          </div>
          <div className="b-links">
            <Link href="/cli/index" className="b-link">
              查看文档
              <Arrow />
            </Link>
            <a
              href="https://github.com/fcbyk/byk"
              target="_blank"
              rel="noreferrer"
              className="b-link b-link--dim"
            >
              GitHub
            </a>
          </div>
        </div>

        {/* Hero terminal — the signature */}
        <div className="b-hero__term">
          <div className="b-term">
            <div className="b-term__bar">
              <span className="b-term__dot b-term__dot--r" />
              <span className="b-term__dot b-term__dot--y" />
              <span className="b-term__dot b-term__dot--g" />
              <span className="b-term__title">~ — byk</span>
            </div>
            <pre className="b-term__body">
              <code>
                <span className="t-p">$ </span>
                <span className="t-c">byk hello</span>
                {'\n'}
                <span className="t-o">hello world</span>
                {'\n\n'}
                <span className="t-p">$ </span>
                <span className="t-c">byk add</span> <span className="t-a">fcbyk/hello</span>
                {'\n'}
                <span className="t-s">✓</span> <span className="t-o">plugin installed · ~/.byk/plugins/hello</span>
                {'\n\n'}
                <span className="t-p">$ </span>
                <span className="t-c">byk dev</span>
                {'\n'}
                <span className="t-x"># running pnpm dev in ~/project/web</span>
              </code>
            </pre>
          </div>
        </div>
      </header>

      {/* ============ Features ============ */}
      <section className="b-shell b-section b-reveal">
        <div className="b-head">
          <span className="b-head__label">核心能力</span>
          <h2 className="b-head__title">一个二进制，按需启用</h2>
          <p className="b-head__sub">
            npm 管理、命令别名、插件系统——都是可选初始化，零隐式副作用。
          </p>
        </div>

        <div className="b-features">
          <Link href="/cli/commands" className="b-feature">
            <span className="b-feature__mark">byk ni</span>
            <h3 className="b-feature__title">npm 命令管理</h3>
            <p className="b-feature__desc">
              在 byk 独立作用域（<span className="b-ic">~/.byk/node-pkgs</span>）下管理
              npm CLI，不污染全局环境。首次 <span className="b-ic">byk add npm</span> 自动创建 ni / nu 别名。
            </p>
            <span className="b-feature__more">
              内置命令 <Arrow />
            </span>
          </Link>

          <Link href="/cli/alias/start" className="b-feature">
            <span className="b-feature__mark">byk ssh.prod</span>
            <h3 className="b-feature__title">命令别名</h3>
            <p className="b-feature__desc">
              把冗长命令集中到 <span className="b-ic">*.byk.json</span>。支持
              <span className="b-ic">{'{xxx}'}</span> 占位符、<span className="b-ic">$cwd</span>、
              <span className="b-ic">$interactive</span> 三级继承。
            </p>
            <span className="b-feature__more">
              开始使用别名 <Arrow />
            </span>
          </Link>

          <Link href="/cli/plugins" className="b-feature">
            <span className="b-feature__mark">byk add</span>
            <h3 className="b-feature__title">插件系统</h3>
            <p className="b-feature__desc">
              从 GitHub 一键安装 Python 插件，在隔离 venv 中运行。支持
              py-script、py-module、pip-bin、bin 四种类型。
            </p>
            <span className="b-feature__more">
              插件协议 <Arrow />
            </span>
          </Link>
        </div>
      </section>

      {/* ============ Alias showcase ============ */}
      <section className="b-shell b-section b-reveal">
        <div className="b-head">
          <span className="b-head__label">命令别名</span>
          <h2 className="b-head__title">用 JSON 描述你的命令集合</h2>
          <p className="b-head__sub">
            一个文件管完项目里所有命令——占位符、交互式输入、精确执行语法都在这里。
          </p>
        </div>

        <div className="b-duo">
          <div className="b-duo__col">
            <p className="b-duo__caption">
              <span className="b-tag">config</span> · run.byk.json
            </p>
            <div className="b-term">
              <div className="b-term__bar">
                <span className="b-term__dot b-term__dot--r" />
                <span className="b-term__dot b-term__dot--y" />
                <span className="b-term__dot b-term__dot--g" />
                <span className="b-term__title">run.byk.json</span>
              </div>
              <pre className="b-term__body b-term__body--sm">
                <code>{`{`}<br />
{`  `}<span className="t-k">{`"$cwd"`}</span>{`: `}<span className="t-v">{`"~/project/web"`}</span>{`,`}<br />
{`  `}<span className="t-k">{`"dev"`}</span>{`:   `}<span className="t-v">{`"pnpm dev"`}</span>{`,`}<br />
{`  `}<span className="t-k">{`"build"`}</span>{`: `}<span className="t-v">{`"pnpm build"`}</span>{`,`}<br />
{`  `}<span className="t-k">{`"deploy"`}</span>{`: {`}<br />
{`    `}<span className="t-k">{`"$cmd"`}</span>{`: `}<span className="t-v">{`"deploy --env {env}"`}</span>{`,`}<br />
{`    `}<span className="t-k">{`"$interactive"`}</span>{`: `}<span className="t-n">{`true`}</span><br />
{`  }`}<br />
{`}`}
                </code>
              </pre>
            </div>
          </div>

          <div className="b-duo__col">
            <p className="b-duo__caption">
              <span className="b-tag">terminal</span> · 执行效果
            </p>
            <div className="b-term">
              <div className="b-term__bar">
                <span className="b-term__dot b-term__dot--r" />
                <span className="b-term__dot b-term__dot--y" />
                <span className="b-term__dot b-term__dot--g" />
                <span className="b-term__title">terminal</span>
              </div>
              <pre className="b-term__body b-term__body--sm">
                <code>
                  <span className="t-p">$ </span>
                  <span className="t-c">byk dev</span>
                  {'\n'}
                  <span className="t-x"># running pnpm dev in ~/project/web</span>
                  {'\n\n'}
                  <span className="t-p">$ </span>
                  <span className="t-c">byk deploy</span>
                  {'\n'}
                  <span className="t-x"># prompts for env, then executes</span>
                  {'\n\n'}
                  <span className="t-p">$ </span>
                  <span className="t-c">byk @release.bump</span> <span className="t-a">patch</span>
                  {'\n'}
                  <span className="t-x"># precise: release.byk.json → bump</span>
                </code>
              </pre>
            </div>
          </div>
        </div>
      </section>

      {/* ============ Plugins ============ */}
      <section className="b-shell b-section b-reveal">
        <div className="b-head">
          <span className="b-head__label">插件</span>
          <h2 className="b-head__title">任何 GitHub 仓库都能成为插件源</h2>
          <p className="b-head__sub">
            根目录放一个 <span className="b-ic">byk.json</span> 即可。四种安装方式：
          </p>
        </div>

        <div className="b-plugins">
          <div className="b-plugin">
            <span className="b-plugin__label">远程仓库</span>
            <p className="b-plugin__cmd">byk add user/repo</p>
            <p className="b-plugin__desc">
              安装 byk.json 中的 $default（或唯一）key
            </p>
          </div>
          <div className="b-plugin">
            <span className="b-plugin__label">指定 key</span>
            <p className="b-plugin__cmd">byk add user/repo/my-key</p>
            <p className="b-plugin__desc">
              当 byk.json 含多个插件时精确指定
            </p>
          </div>
          <div className="b-plugin">
            <span className="b-plugin__label">指定版本</span>
            <p className="b-plugin__cmd">byk add user/repo@v1.0/hello</p>
            <p className="b-plugin__desc">@ 语法指定 branch / tag / commit</p>
          </div>
          <div className="b-plugin">
            <span className="b-plugin__label">CDN 加速</span>
            <p className="b-plugin__cmd">byk add --cdn user/repo</p>
            <p className="b-plugin__desc">
              raw.githubusercontent.com → jsDelivr
            </p>
          </div>
        </div>
      </section>

      {/* ============ Why byk ============ */}
      <section className="b-shell b-section b-reveal">
        <div className="b-head">
          <span className="b-head__label">为什么选 byk</span>
          <h2 className="b-head__title">为不喜欢副作用的人设计</h2>
        </div>

        <div className="b-why">
          <div className="b-why__item">
            <div className="b-why__term">
              <span className="b-idx">01</span>
              <strong>按需启用</strong>
            </div>
            <p>
              npm / 补全 / 缓存 / venv 都是可选初始化（<span className="b-ic">byk add &lt;feature&gt;</span>），无任何隐式副作用。
            </p>
          </div>
          <div className="b-why__item">
            <div className="b-why__term">
              <span className="b-idx">02</span>
              <strong>隔离环境</strong>
            </div>
            <p>
              插件运行在专属 venv（<span className="b-ic">~/.byk/venv</span>），与系统 pip 隔离；别名和 npm 也有独立作用域。
            </p>
          </div>
          <div className="b-why__item">
            <div className="b-why__term">
              <span className="b-idx">03</span>
              <strong>跨平台</strong>
            </div>
            <p>
              Rust 编译产物，原生支持 macOS / Linux / Windows（x86_64 / arm64）。
            </p>
          </div>
          <div className="b-why__item">
            <div className="b-why__term">
              <span className="b-idx">04</span>
              <strong>零 Python 依赖</strong>
            </div>
            <p>
              Shell 脚本一键安装即可使用 byk 本体；需要插件时才要求 Python。
            </p>
          </div>
          <div className="b-why__item">
            <div className="b-why__term">
              <span className="b-idx">05</span>
              <strong>生态开放</strong>
            </div>
            <p>
              byk.json 协议简洁；支持 Ref 引用、$var 变量替换、$default 默认 key、跨仓库拆分。
            </p>
          </div>
          <div className="b-why__item">
            <div className="b-why__term">
              <span className="b-idx">06</span>
              <strong>清晰的优先级</strong>
            </div>
            <p>
              命令按 内置 → 全局选项 → 插件 → NPM → @file.key → 别名 顺序匹配，第一个命中即执行。
            </p>
          </div>
        </div>
      </section>

      {/* ============ Closing ============ */}
      <section className="b-shell b-close b-reveal">
        <div className="b-close__wordmark">
          byk<span className="b-caret" aria-hidden="true" />
        </div>
        <p className="b-close__sub">几秒钟安装，立刻提升你的终端效率</p>
        <div className="b-close__actions">
          <Link href="/cli/index" className="b-btn b-btn--solid">
            查看 CLI 文档
            <Arrow />
          </Link>
          <a
            href="https://github.com/fcbyk/byk"
            target="_blank"
            rel="noreferrer"
            className="b-btn b-btn--outline"
          >
            <GitHubIcon />
            Star on GitHub
          </a>
        </div>
      </section>
    </main>
  );
}

function Arrow() {
  return (
    <svg
      viewBox="0 0 24 24"
      width="15"
      height="15"
      fill="none"
      stroke="currentColor"
      strokeWidth="2.2"
      strokeLinecap="round"
      strokeLinejoin="round"
      aria-hidden="true"
    >
      <path d="M5 12h14M13 6l6 6-6 6" />
    </svg>
  );
}

function GitHubIcon() {
  return (
    <svg
      viewBox="0 0 24 24"
      width="16"
      height="16"
      fill="currentColor"
      aria-hidden="true"
    >
      <path d="M12 .5C5.65.5.5 5.65.5 12c0 5.08 3.29 9.39 7.86 10.91.58.1.79-.25.79-.56v-2c-3.2.69-3.87-1.37-3.87-1.37-.52-1.33-1.27-1.68-1.27-1.68-1.04-.71.08-.7.08-.7 1.15.08 1.76 1.18 1.76 1.18 1.02 1.76 2.69 1.25 3.35.96.1-.74.4-1.25.72-1.54-2.55-.29-5.24-1.28-5.24-5.69 0-1.26.45-2.29 1.18-3.1-.12-.29-.51-1.47.11-3.06 0 0 .97-.31 3.17 1.18a11 11 0 0 1 5.78 0c2.2-1.49 3.17-1.18 3.17-1.18.62 1.59.23 2.77.11 3.06.73.81 1.18 1.84 1.18 3.1 0 4.42-2.69 5.39-5.26 5.68.41.36.78 1.07.78 2.16v3.2c0 .31.21.67.8.56C20.21 21.39 23.5 17.08 23.5 12 23.5 5.65 18.35.5 12 .5Z" />
    </svg>
  );
}

export default BykHome;
