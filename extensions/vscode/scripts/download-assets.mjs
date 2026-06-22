#!/usr/bin/env node
/**
 * Download static assets from GitHub Releases (assets tag).
 *
 * Usage:
 *   node scripts/download-assets.mjs
 */

import { createWriteStream } from 'node:fs';
import { mkdir } from 'node:fs/promises';
import { get } from 'node:https';
import { join, dirname } from 'node:path';
import { fileURLToPath } from 'node:url';

const REPO = process.env.BYK_REPO || 'fcbyk/byk';
const __dirname = dirname(fileURLToPath(import.meta.url));
const ASSETS_DIR = join(__dirname, '..', 'assets');

const BASE_URL = `https://github.com/${REPO}/releases/download/assets`;

const ASSETS = [
  'logo.png',
];

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

async function main() {
  console.log(`⬇  ${REPO} assets`);

  await mkdir(ASSETS_DIR, { recursive: true });

  for (const asset of ASSETS) {
    const url = `${BASE_URL}/${asset}`;
    const dest = join(ASSETS_DIR, asset);

    console.log(`   ${asset}`);
    await downloadFile(url, dest);
  }

  console.log('✅ Done');
}

main().catch((err) => {
  console.error('❌', err.message);
  process.exit(1);
});