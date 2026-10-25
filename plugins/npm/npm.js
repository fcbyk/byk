#!/usr/bin/env node
/**
 * npm 插件：管理系统级 npm 包，并自动注册/注销其 bin 命令。
 *
 * 用法（通过 byk 调用）：
 *   byk npm i <pkg>     安装包并注册其 bin 命令
 *   byk npm uni <pkg>   卸载包并注销对应命令
 *   byk npm ls          列出已注册的 npm 命令
 *
 * 设计约定：
 *   - 包统一安装到 ~/.byk/node-pkgs/，bin 目录为 node-pkgs/node_modules/.bin
 *   - 路径写死为 ~/.byk（不依赖 byk 注入环境变量）
 *   - 仓库源管理：use/source 子命令读写 node-pkgs/.npmrc 的 registry= 行，
 *     i 的 -r/--registry 一次性覆盖；npm 原生 .npmrc 是唯一事实源
 *   - 自愈同步：package.json 的直接依赖（dependencies）是唯一事实源，
 *     不含间接依赖；每次调用对齐别名文件（补注册 / 刷描述 / 剪枝）；
 *     clean 显式清空包与注册
 *   - 动态读写 ~/.byk/alias/npm-pkgs.byk.json（一个平铺的别名文件），
 *     每条 bin 命令 = { $cmd: "$byk/node-pkgs/node_modules/.bin/<名>", $description }
 *   - 读-改-写整个文件；原子写（临时文件 + rename）
 *   - 零依赖，仅使用 node 内置模块
 */
'use strict';

const fs = require('fs');
const os = require('os');
const path = require('path');
const { spawnSync } = require('child_process');

// ---------------------------------------------------------------------------
// 路径（写死 ~/.byk，不依赖 byk 注入环境变量）
// ---------------------------------------------------------------------------

const BYK_HOME = path.join(os.homedir(), '.byk');
const NODE_PKGS = path.join(BYK_HOME, 'node-pkgs');
const BIN_DIR = path.join(NODE_PKGS, 'node_modules', '.bin');
const ALIAS_FILE = path.join(BYK_HOME, 'alias', 'npm-pkgs.byk.json');

// Windows 上 npm 是 npm.cmd，无 shell 直接 spawn 需要显式扩展名
const NPM_BIN = process.platform === 'win32' ? 'npm.cmd' : 'npm';

const NPMRC = path.join(NODE_PKGS, '.npmrc');
const DEFAULT_REGISTRY = 'https://registry.npmjs.org';

/// 仓库源预设：名称 → 地址
const REGISTRY_PRESETS = {
  official: 'https://registry.npmjs.org',
  npmmirror: 'https://registry.npmmirror.com',
};

/// 预设别名 → 规范名
const REGISTRY_ALIASES = {
  npm: 'official',
  taobao: 'npmmirror',
};

// ---------------------------------------------------------------------------
// 基础工具
// ---------------------------------------------------------------------------

function info(msg) {
  console.log(msg);
}

function die(msg) {
  console.error(`Error: ${msg}`);
  process.exit(1);
}

/// 读 JSON 文件；不存在返回 null，损坏则报错退出
function readJson(file) {
  let text;
  try {
    text = fs.readFileSync(file, 'utf8');
  } catch (e) {
    if (e.code === 'ENOENT') return null;
    die(`failed to read ${file}: ${e.message}`);
  }
  try {
    return JSON.parse(text);
  } catch (e) {
    die(`failed to parse ${file}: ${e.message}`);
  }
}

/// 原子写 JSON（临时文件 + rename），格式与 byk 的 serde pretty 一致；
/// 父目录不存在时自动创建（与 byk json_io::write_json 行为一致）
function writeJson(file, data) {
  fs.mkdirSync(path.dirname(file), { recursive: true });
  const tmp = `${file}.tmp`;
  fs.writeFileSync(tmp, JSON.stringify(data, null, 2));
  fs.renameSync(tmp, file);
}

/// 运行 npm，继承 stdio 让用户看到安装进度；返回退出码
function npm(args) {
  const r = spawnSync(NPM_BIN, args, { stdio: 'inherit', cwd: NODE_PKGS });
  if (r.error) {
    die(`failed to run npm: ${r.error.message} (is npm installed and in PATH?)`);
  }
  return r.status === null ? 1 : r.status;
}

/// 读 JSON 文件，任何失败都返回 null（用于扫描不可控的 node_modules）
function readJsonSoft(file) {
  try {
    return JSON.parse(fs.readFileSync(file, 'utf8'));
  } catch {
    return null;
  }
}

/// 从安装规格中提取包名（去掉 @version 后缀）
///   "eslint@8" → "eslint"；"@antfu/ni@30.6.0" → "@antfu/ni"；"eslint" → "eslint"
function pkgNameFromSpec(spec) {
  const at = spec.lastIndexOf('@');
  return at > 0 ? spec.slice(0, at) : spec;
}

/// 判断是否为本地路径规格（./x、../x、/abs/x、file:x）
function isLocalPath(spec) {
  const s = spec.replace(/^file:/, '');
  return s.startsWith('.') || s.startsWith('/') || path.isAbsolute(s);
}

/// 解析已安装包的 package.json 与包名。
/// 普通规格（eslint@8）按去版本名查找；本地路径安装后包名取自
/// 源目录 package.json 的 name 字段，而非路径本身。
function resolveInstalledPkg(spec) {
  let name = pkgNameFromSpec(spec);
  let data = readJson(path.join(NODE_PKGS, 'node_modules', name, 'package.json'));
  if (!data && isLocalPath(spec)) {
    const srcJson = readJson(path.resolve(spec.replace(/^file:/, ''), 'package.json'));
    if (srcJson && srcJson.name) {
      name = srcJson.name;
      data = readJson(path.join(NODE_PKGS, 'node_modules', name, 'package.json'));
    }
  }
  if (!data) {
    die(`package "${spec}" is not installed in ${NODE_PKGS} (resolved name: "${name}")`);
  }
  return { pkgJson: data, name: data.name || name };
}

/// 从 package.json 提取 bin 命令名：
///   bin 为字符串 → 命令名取包名末段（@scope/name → name）
///   bin 为对象   → 取所有 key
function binNames(pkgJson, pkgName) {
  const bin = pkgJson && pkgJson.bin;
  if (!bin) return [];
  if (typeof bin === 'string') {
    return [pkgName.split('/').pop()];
  }
  if (typeof bin === 'object') {
    return Object.keys(bin);
  }
  return [];
}

// ---------------------------------------------------------------------------
// 仓库源（.npmrc 管理）
// ---------------------------------------------------------------------------

/// 解析仓库源参数：预设名（含别名）或完整 URL；非法则报错并列出自预设
function resolveSource(nameOrUrl) {
  if (/^https?:\/\//.test(nameOrUrl)) {
    return nameOrUrl.replace(/\/+$/, '');
  }
  const key = REGISTRY_ALIASES[nameOrUrl] || nameOrUrl;
  const url = REGISTRY_PRESETS[key];
  if (!url) {
    const names = Object.keys(REGISTRY_PRESETS).join(', ');
    die(`unknown registry source "${nameOrUrl}"\n  presets: ${names}\n  or pass a full URL (https://...)`);
  }
  return url;
}

/// URL 对应的规范预设名；非预设地址返回 null
function canonicalPresetName(url) {
  for (const [name, u] of Object.entries(REGISTRY_PRESETS)) {
    if (u === url) return name;
  }
  return null;
}

/// 读 .npmrc 中的默认 registry；未设置返回 null
function readRegistry() {
  let text;
  try {
    text = fs.readFileSync(NPMRC, 'utf8');
  } catch (e) {
    if (e.code === 'ENOENT') return null;
    die(`failed to read ${NPMRC}: ${e.message}`);
  }
  for (const line of text.split(/\r?\n/)) {
    const m = line.match(/^\s*registry\s*=\s*(\S+)\s*$/);
    if (m) return m[1];
  }
  return null;
}

/// 写入默认 registry：保留 .npmrc 其余行（auth token 等），替换/追加 registry= 行
function setRegistry(url) {
  let lines = [];
  try {
    lines = fs.readFileSync(NPMRC, 'utf8').split(/\r?\n/);
  } catch (e) {
    if (e.code !== 'ENOENT') die(`failed to read ${NPMRC}: ${e.message}`);
  }
  let replaced = false;
  const out = [];
  for (const line of lines) {
    if (/^\s*registry\s*=/.test(line)) {
      if (replaced) continue; // 丢弃多余的 registry= 行，避免歧义
      replaced = true;
      out.push(`registry=${url}`);
    } else {
      out.push(line);
    }
  }
  if (!replaced) out.push(`registry=${url}`);
  fs.mkdirSync(NODE_PKGS, { recursive: true });
  fs.writeFileSync(NPMRC, `${out.join('\n')}\n`);
}

// ---------------------------------------------------------------------------
// 状态文件读写
// ---------------------------------------------------------------------------

/// 读别名文件（~/.byk/alias/npm-pkgs.byk.json）；不存在/损坏返回空对象
function loadAliases() {
  const data = readJson(ALIAS_FILE);
  return data && typeof data === 'object' && !Array.isArray(data) ? data : {};
}

// ---------------------------------------------------------------------------
// 自愈同步：node-pkgs 磁盘状态 → 命令注册表
// ---------------------------------------------------------------------------

/// 收集单个已安装包的 bin 命令到 expected（name → desc）
function collectPackage(pkgDir, expected) {
  const data = readJsonSoft(path.join(pkgDir, 'package.json'));
  if (!data || !data.name) return;
  const desc = `from ${data.name}@${data.version || 'unknown'}`;
  for (const name of binNames(data, data.name)) {
    expected.set(name, desc);
  }
}

/// 以 package.json 的直接依赖为唯一事实源，对齐别名文件：
/// - 依赖已安装且有 bin：未注册则注册，已注册则刷新描述（版本变化）
/// - 别名文件中已不是直接依赖的：剪掉（如 uni 之后、手动改动）
/// 只认 dependencies，不扫整个 node_modules——间接依赖（传递依赖）
/// 不注册，哪怕它带 bin（如 vite 的 rolldown / nanoid）。
/// 文件级 $paths 前置 bin 目录，让每条 bin 命令的 $cmd 只写命令名。
/// 返回汇总 { registered, updated, pruned }
function syncRegistrations() {
  // 1. 读取直接依赖，构建期望注册表（bin 命令名 → desc）
  const expected = new Map();
  const rootPkg = readJsonSoft(path.join(NODE_PKGS, 'package.json'));
  const deps =
    rootPkg && rootPkg.dependencies && typeof rootPkg.dependencies === 'object'
      ? rootPkg.dependencies
      : {};
  for (const depName of Object.keys(deps)) {
    // 依赖键即包名（本地路径安装时 npm 也以包名作为键）；
    // 未实际安装时 collectPackage 静默跳过
    collectPackage(path.join(NODE_PKGS, 'node_modules', depName), expected);
  }

  // 2. 期望的别名文件：$paths 前置 bin 目录 + 每条 bin 命令一个别名（$cmd 只写命令名）
  const next = {};
  if (expected.size > 0) {
    next['$paths'] = ['$byk/node-pkgs/node_modules/.bin'];
    for (const [name, desc] of expected) {
      next[name] = { $cmd: name, $description: desc };
    }
  }

  // 3. 与当前文件对比，汇总差异（registered / updated / pruned）
  const aliases = loadAliases();
  const registered = [];
  const updated = [];
  for (const [name, desc] of expected) {
    const existing = aliases[name];
    if (!existing) {
      registered.push(name);
    } else if (existing.$description !== desc) {
      updated.push(name);
    }
  }
  const pruned = [];
  for (const name of Object.keys(aliases)) {
    if (!name.startsWith('$') && !expected.has(name)) {
      pruned.push(name);
    }
  }

  // 4. 有变化才落盘
  if (JSON.stringify(next) !== JSON.stringify(aliases)) {
    writeJson(ALIAS_FILE, next);
  }

  return { registered, updated, pruned };
}

/// 打印同步汇总（只打非空项）
function reportSync(r) {
  if (r.registered.length > 0) info(`Registered: ${r.registered.join(', ')}`);
  if (r.updated.length > 0) info(`Updated:    ${r.updated.join(', ')}`);
  if (r.pruned.length > 0) {
    info(`Pruned:     ${r.pruned.join(', ')} (package no longer installed)`);
  }
}

// ---------------------------------------------------------------------------
// 子命令
// ---------------------------------------------------------------------------

/// 解析 i 的参数：提取 -r/--registry <源>，其余第一个非 flag 参数为包规格
function parseInstallArgs(args) {
  let registry = null;
  let spec = null;
  for (let i = 0; i < args.length; i++) {
    const a = args[i];
    if (a === '-r' || a === '--registry') {
      registry = args[i + 1];
      if (!registry) die('-r/--registry requires a value (preset name or URL)');
      i += 1;
    } else if (a.startsWith('--registry=')) {
      registry = a.slice('--registry='.length);
    } else if (a.startsWith('-')) {
      die(`unsupported option: ${a}\n  only -r/--registry is supported here; run npm directly for other flags`);
    } else if (spec) {
      die(`unexpected extra argument: ${a}`);
    } else {
      spec = a;
    }
  }
  return { spec, registry };
}

/// byk npm i [-r <源>] <pkg>：安装包并注册其 bin 命令
function cmdInstall(args) {
  const { spec, registry } = parseInstallArgs(args);
  if (!spec) {
    usage();
    process.exit(1);
  }

  fs.mkdirSync(NODE_PKGS, { recursive: true });

  // 仓库源优先级：-r 一次性覆盖 > .npmrc 持久设置 > npm 默认
  const npmArgs = ['install', spec];
  if (registry) {
    const url = resolveSource(registry);
    npmArgs.push('--registry', url);
    info(`Using registry: ${url} (one-off)`);
  } else {
    const current = readRegistry();
    info(`Using registry: ${current || `${DEFAULT_REGISTRY} (npm default)`}`);
  }

  info(`Installing ${spec} into ${NODE_PKGS} ...`);
  const code = npm(npmArgs);
  if (code !== 0) process.exit(code);

  // 安装后自愈同步：磁盘 → 注册表（新包的 bin 自动注册）
  reportSync(syncRegistrations());
}

/// byk npm use <preset|url>：持久切换仓库源（写 .npmrc）
function cmdUse(nameOrUrl) {
  if (!nameOrUrl) {
    usage();
    process.exit(1);
  }
  const url = resolveSource(nameOrUrl);
  setRegistry(url);
  const name = canonicalPresetName(url);
  info(`Registry set to: ${url}${name ? ` (${name})` : ''}`);
  info('Note: existing packages are not migrated; the new source applies to future installs.');
}

/// byk npm source：查看当前仓库源与预设
function cmdSource() {
  const current = readRegistry();
  info(`Current registry: ${current || `${DEFAULT_REGISTRY} (npm default, not set in .npmrc)`}`);
  info('Presets:');
  for (const [name, url] of Object.entries(REGISTRY_PRESETS)) {
    const aliases = Object.keys(REGISTRY_ALIASES)
      .filter((a) => REGISTRY_ALIASES[a] === name)
      .join(', ');
    const suffix = aliases ? `  (alias: ${aliases})` : '';
    info(`  ${name.padEnd(10)} ${url}${suffix}`);
  }
  info('Switch:  byk npm use <preset|url>');
  info('One-off: byk npm i -r <preset|url> <pkg>');
}

/// byk npm uni <pkg>：卸载包并注销对应命令
function cmdUninstall(spec) {
  if (!spec) {
    usage();
    process.exit(1);
  }

  // 先确认包已安装（给出清晰错误），再卸载，最后自愈同步剪掉命令
  const { name: pkgName } = resolveInstalledPkg(spec);

  info(`Uninstalling ${pkgName} ...`);
  const code = npm(['uninstall', pkgName]);
  if (code !== 0) process.exit(code);

  reportSync(syncRegistrations());
}

/// byk npm ls：列出本插件注册的命令（先自愈同步）
function cmdList() {
  reportSync(syncRegistrations());

  const aliases = loadAliases();
  const mine = Object.keys(aliases)
    .filter((name) => !name.startsWith('$'))
    .sort();
  if (mine.length === 0) {
    info('No npm commands registered.');
    return;
  }
  info(`Registered npm commands (${mine.length}):`);
  for (const name of mine) {
    info(`  ${name}  ${aliases[name].$description || ''}`);
  }
}

/// byk npm clean：清空所有已安装包并注销全部本插件命令。
/// 保留 .npmrc（仓库源偏好与安装的包无关）。
function cmdClean() {
  const aliases = loadAliases();
  const ours = Object.keys(aliases).filter((name) => !name.startsWith('$'));
  writeJson(ALIAS_FILE, {});

  // 清理包文件；保留 .npmrc
  for (const target of ['node_modules', 'package.json', 'package-lock.json']) {
    fs.rmSync(path.join(NODE_PKGS, target), { recursive: true, force: true });
  }

  info(`Unregistered ${ours.length} command(s): ${ours.join(', ') || '(none)'}`);
  info(`Cleared packages under ${NODE_PKGS} (registry config ${NPMRC} kept)`);
}

function usage() {
  info(`npm plugin - manage npm packages and their bin commands

Usage:
  byk npm i [-r <source>] <pkg>  Install a package and register its bin commands
                                 (-r: one-off registry, preset name or URL)
  byk npm uni <pkg>              Uninstall a package and unregister its commands
  byk npm ls                     List registered npm commands (self-heals state)
  byk npm clean                  Remove ALL packages and unregister ALL commands
  byk npm use <preset|url>       Switch registry source (persisted in .npmrc)
  byk npm source                 Show current registry and presets

Packages are installed under: ${NODE_PKGS}
Bin commands are registered as aliases in: ${ALIAS_FILE}
Registry config file: ${NPMRC}`);
}

// ---------------------------------------------------------------------------
// 入口
// ---------------------------------------------------------------------------

const [sub, ...rest] = process.argv.slice(2);
switch (sub) {
  case 'i':
  case 'install':
    cmdInstall(rest);
    break;
  case 'uni':
  case 'uninstall':
    cmdUninstall(rest[0]);
    break;
  case 'ls':
  case 'list':
    cmdList();
    break;
  case 'clean':
    cmdClean();
    break;
  case 'use':
    cmdUse(rest[0]);
    break;
  case 'source':
    cmdSource();
    break;
  case undefined:
    usage();
    break;
  default:
    usage();
    process.exit(1);
}
