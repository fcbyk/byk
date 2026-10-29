import { useState } from 'react';
import { Link } from '@rspress/core/theme';
import './home.css';

/**
 * byk docs — 自定义首页
 *
 * 设计方向：terminal-native。
 * byk 是终端工具，只有一个概念：别名。
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
          <span>v0.5.1</span>
          <span style={{ opacity: 0.4 }}>·</span>
          <span>MIT</span>
          <span style={{ opacity: 0.4 }}>·</span>
          <span>Rust</span>
        </div>

        <h1 className="b-hero__wordmark">
          byk<span className="b-caret" aria-hidden="true" />
        </h1>

        <p className="b-hero__lede">一个只会执行别名的 CLI</p>
        <p className="b-hero__sub">
          命令写进 <span className="b-ic">*.byk.json</span>，短名字调用
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
                <span className="t-c">byk @run.dev</span>
                {'\n'}
                <span className="t-o">pnpm dev</span>
                <span className="t-x"> ← in ~/project/web</span>
                {'\n\n'}
                <span className="t-p">$ </span>
                <span className="t-c">byk @run.deploy</span>
                {'\n'}
                <span className="t-x">? env › </span>
                <span className="t-a">staging</span>
                {'\n'}
                <span className="t-s">✓</span> <span className="t-o">deploy --env staging</span>
                {'\n\n'}
                <span className="t-p">$ </span>
                <span className="t-c">byk -h</span>
                {'\n'}
                <span className="t-x">Aliases:</span>
                {'\n'}
                <span className="t-x">  run.dev    </span>
                <span className="t-o">pnpm dev</span>
                {'\n'}
                <span className="t-x">  run.build  </span>
                <span className="t-o">pnpm build</span>
              </code>
            </pre>
          </div>
        </div>
      </header>

      {/* ============ Features ============ */}
      <section className="b-shell b-section b-reveal">
        <div className="b-head">
          <span className="b-head__label">核心</span>
          <h2 className="b-head__title">写 JSON，敲短命令</h2>
          <p className="b-head__sub">
            一个文件说清项目里所有命令，调用时不用记完整路径和参数
          </p>
        </div>

        <div className="b-features">
          <Link href="/cli/alias/config" className="b-feature">
            <span className="b-feature__mark">run.byk.json</span>
            <h3 className="b-feature__title">命令别名</h3>
            <p className="b-feature__desc">
              <span className="b-ic">{'{xxx}'}</span> 占位符、
              <span className="b-ic">$cwd</span>、
              <span className="b-ic">$interactive</span> 三级继承，
              <span className="b-ic">$var</span> 变量与平台映射。
            </p>
            <span className="b-feature__more">
              别名配置 <Arrow />
            </span>
          </Link>

          <Link href="/cli/commands" className="b-feature">
            <span className="b-feature__mark">byk @run.build</span>
            <h3 className="b-feature__title">精确执行</h3>
            <p className="b-feature__desc">
              本地文件默认不参与合并且
              <span className="b-ic">@文件.key</span>
              直接路由到那个文件，进入任何目录都不会撞名。
            </p>
            <span className="b-feature__more">
              选项与命令 <Arrow />
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
                <code>{'{'}<br />
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
                  <span className="t-c">byk @run.dev</span>
                  {'\n'}
                  <span className="t-x"># running pnpm dev in ~/project/web</span>
                  {'\n\n'}
                  <span className="t-p">$ </span>
                  <span className="t-c">byk @run.deploy</span>
                  {'\n'}
                  <span className="t-x"># prompts for env, then executes</span>
                  {'\n\n'}
                  <span className="t-p">$ </span>
                  <span className="t-c">byk @run.build</span> <span className="t-a">--mode prod</span>
                  {'\n'}
                  <span className="t-x"># args are appended to the command</span>
                </code>
              </pre>
            </div>
          </div>
        </div>
      </section>

      {/* ============ CLI surface ============ */}
      <section className="b-shell b-section b-reveal">
        <div className="b-head">
          <span className="b-head__label">命令行</span>
          <h2 className="b-head__title">三个选项，一个接口</h2>
          <p className="b-head__sub">
            没有子命令。除了别名，byk 只有这些。
          </p>
        </div>

        <div className="b-tiles">
          <div className="b-tile">
            <span className="b-tile__label">帮助</span>
            <p className="b-tile__cmd">byk -h</p>
            <p className="b-tile__desc">
              banner、用法、选项和全部别名一览；不带参数等价
            </p>
          </div>
          <div className="b-tile">
            <span className="b-tile__label">版本</span>
            <p className="b-tile__cmd">byk -v</p>
            <p className="b-tile__desc">
              版本、commit、构建日期、平台与安装位置
            </p>
          </div>
          <div className="b-tile">
            <span className="b-tile__label">路径</span>
            <p className="b-tile__cmd">byk --paths</p>
            <p className="b-tile__desc">
              CLI 家目录与全局别名目录的实际位置
            </p>
          </div>
          <div className="b-tile">
            <span className="b-tile__label">程序接口</span>
            <p className="b-tile__cmd">byk __api__</p>
            <p className="b-tile__desc">
              只读、不写盘：给 shell 补全和编辑器插件取数据
            </p>
          </div>
        </div>
      </section>

      {/* ============ Why byk ============ */}
      <section className="b-shell b-section b-reveal">
        <div className="b-head">
          <span className="b-head__label">为什么选 byk</span>
          <h2 className="b-head__title">只把命令收进一个入口</h2>
        </div>

        <div className="b-why">
          <div className="b-why__item">
            <div className="b-why__term">
              <span className="b-idx">01</span>
              <strong>来源可预期</strong>
            </div>
            <p>
              选项 → 隐藏接口 → <span className="b-ic">@file.key</span> → 别名，第一个命中即执行；
              未命中时列出候选并退出 1。
            </p>
          </div>
          <div className="b-why__item">
            <div className="b-why__term">
              <span className="b-idx">02</span>
              <strong>作用域清晰</strong>
            </div>
            <p>
              本地文件默认 <span className="b-ic">$priority: -1</span>，不参与合并；
              要全局可用就放进 <span className="b-ic">~/.byk/alias/</span>。
            </p>
          </div>
          <div className="b-why__item">
            <div className="b-why__term">
              <span className="b-idx">03</span>
              <strong>跨平台</strong>
            </div>
            <p>
              Rust 编译的单个二进制，原生支持 macOS / Linux / Windows（x86_64 / arm64）。
            </p>
          </div>
          <div className="b-why__item">
            <div className="b-why__term">
              <span className="b-idx">04</span>
              <strong>零依赖</strong>
            </div>
            <p>
              装的就只有 byk 本体。别名执行的是你 shell 里本来就能跑的命令。
            </p>
          </div>
          <div className="b-why__item">
            <div className="b-why__term">
              <span className="b-idx">05</span>
              <strong>只有一种配置</strong>
            </div>
            <p>
              <span className="b-ic">*.byk.json</span> 里五个 <span className="b-ic">$</span> 字段，
              没有插件协议，没有安装清单，没有环境目录。
            </p>
          </div>
          <div className="b-why__item">
            <div className="b-why__term">
              <span className="b-idx">06</span>
              <strong>状态只有别名</strong>
            </div>
            <p>
              <span className="b-ic">~/.byk/</span> 里除了别名文件和一个扫描缓存，
              不产生别的东西。
            </p>
          </div>
        </div>
      </section>

      {/* ============ Closing ============ */}
      <section className="b-shell b-close b-reveal">
        <div className="b-close__wordmark">
          byk<span className="b-caret" aria-hidden="true" />
        </div>
        <p className="b-close__sub">几秒钟安装，把常用命令收成短名字</p>
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
