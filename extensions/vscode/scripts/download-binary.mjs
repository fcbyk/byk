#!/usr/bin/env node
/**
 * Download byk binaries from GitHub Releases (v2 artifact naming).
 * All archives contain a single `byk` / `byk.exe` — no renames needed.
 *
 * Usage:
 *   node scripts/download-binary.mjs              # tag from package.json
 *   node scripts/download-binary.mjs v0.6.0       # specific tag
 *
 * Version source: package.json `bykBinaryVersion`, falling back to `version`.
 * The plugin's own version tracks the CLI's major.minor while its patch digit
 * increments independently, so `bykBinaryVersion` is the only field that pins
 * the exact binary. Never default to `latest` — that makes packaging
 * non-reproducible and detaches the bundled binary from the declared version.
 *
 * Env:
 *   BYK_REPO   GitHub repo (default: fcbyk/byk)
 */

import { execSync } from 'node:child_process';
import { createWriteStream, readFileSync } from 'node:fs';
import { mkdir, rm } from 'node:fs/promises';
import { get } from 'node:https';
import { join, dirname } from 'node:path';
import { fileURLToPath } from 'node:url';

// ── Config ──────────────────────────────────────────────────────────

const REPO = process.env.BYK_REPO || 'fcbyk/byk';
const __dirname = dirname(fileURLToPath(import.meta.url));
const PKG_DIR = join(__dirname, '..');
const BIN_DIR = join(PKG_DIR, 'bin');
const TMP_DIR = join(BIN_DIR, '.tmp');

/** Resolve the release tag from package.json. */
function resolveDefaultTag() {
  const pkg = JSON.parse(readFileSync(join(PKG_DIR, 'package.json'), 'utf8'));
  const version = pkg.bykBinaryVersion || pkg.version;
  if (!version) {
    throw new Error('package.json has neither bykBinaryVersion nor version');
  }
  return `v${version}`;
}


// Asset name → bin/ output directory
// Asset arch uses x86_64; bin dir uses x64 (matching process.arch)
const PLATFORMS = {
  'byk-darwin-arm64.tar.gz':   'darwin-arm64',
  'byk-darwin-x86_64.tar.gz':  'darwin-x64',
  'byk-linux-arm64.tar.gz':    'linux-arm64',
  'byk-linux-x86_64.tar.gz':   'linux-x64',
  'byk-windows-x64.zip':       'win32-x64',
};

// ── Helpers ─────────────────────────────────────────────────────────

function downloadFile(url, dest) {
  return new Promise((resolve, reject) => {
    const file = createWriteStream(dest);
    const req = get(url, {
      headers: { 'User-Agent': 'byk-vscode-downloader' },
    }, (res) => {
      if (res.statusCode >= 300 && res.statusCode < 400 && res.headers.location) {
        file.close();
        req.destroy();
        return downloadFile(res.headers.location, dest).then(resolve, reject);
      }
      if (res.statusCode >= 400) {
        file.close();
        req.destroy();
        return reject(new Error(`HTTP ${res.statusCode} ${url}`));
      }
      res.pipe(file);
      file.on('finish', () => {
        req.destroy();
        resolve();
      });
      file.on('error', (e) => {
        req.destroy();
        reject(e);
      });
    });
    req.on('error', (e) => {
      file.close();
      reject(e);
    });
  });
}

// ── Main ────────────────────────────────────────────────────────────

async function main() {
  const version = process.argv[2] || resolveDefaultTag();
  const tag = version.startsWith('v') ? version : `v${version}`;

  const baseUrl = `https://github.com/${REPO}/releases/download/${tag}`;

  console.log(`⬇  ${REPO} @ ${tag}`);

  await rm(BIN_DIR, { recursive: true, force: true });
  await mkdir(TMP_DIR, { recursive: true });

  for (const [asset, dir] of Object.entries(PLATFORMS)) {
    const url = `${baseUrl}/${asset}`;
    const targetDir = join(BIN_DIR, dir);
    const tmpFile = join(TMP_DIR, asset);

    console.log(`   ${asset}`);
    await downloadFile(url, tmpFile);

    await mkdir(targetDir, { recursive: true });

    if (asset.endsWith('.zip')) {
      execSync(`unzip -qo "${tmpFile}" -d "${targetDir}"`, { stdio: 'pipe' });
    } else {
      execSync(`tar xzf "${tmpFile}" -C "${targetDir}"`, { stdio: 'pipe' });
    }

    if (!dir.startsWith('win')) {
      execSync(`chmod +x "${join(targetDir, 'byk')}"`, { stdio: 'pipe' });
    }
  }

  await rm(TMP_DIR, { recursive: true, force: true });
  console.log('✅ Done');
}

main().catch((e) => {
  console.error(e.message || e);
  process.exit(1);
});