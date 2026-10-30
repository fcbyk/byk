#!/usr/bin/env node
/**
 * Validate the two version axes declared in package.json.
 *
 * The extension carries two version numbers:
 *   version            X.Y.Z  the extension itself
 *   bykBinaryVersion   X.Y.Z  the exact CLI release bundled into this build
 *
 * Rules enforced here:
 *   1. Both are plain semver triples (a `-prerelease` suffix is tolerated).
 *   2. Their major.minor must match — that is what "X.Y follows the CLI" means.
 *      The patch digit of bykBinaryVersion may lag any number of releases
 *      behind, because the extension absorbs upstream at its own pace.
 *   3. bykBinaryVersion never goes backwards, so a newer extension build can
 *      never ship an older CLI. Checked against git history when available.
 *
 * Bumping the extension to 0.8.0 without bumping bykBinaryVersion is the easy
 * mistake this catches: it would ship a build claiming the 0.8 line while
 * actually running a 0.7 CLI.
 *
 * Escape hatch for a deliberate rollback (a broken CLI release):
 *   BYK_ALLOW_BINARY_DOWNGRADE=1 pnpm run package
 *
 * Usage:
 *   node scripts/verify-version.mjs [path/to/package.json]
 */

import { execFileSync } from 'node:child_process';
import { readFileSync, realpathSync } from 'node:fs';
import { dirname, join, relative, resolve, sep } from 'node:path';
import { fileURLToPath } from 'node:url';

const __dirname = dirname(fileURLToPath(import.meta.url));
const DEFAULT_PKG = join(__dirname, '..', 'package.json');
const SEMVER = /^\d+\.\d+\.\d+(?:-[0-9A-Za-z.-]+)?$/;

function fail(message) {
  console.error(`✖ ${message}`);
  process.exit(1);
}

function note(message) {
  console.log(`  ${message}`);
}

/** Strip any prerelease suffix, returning the numeric core. */
function core(version) {
  return version.split('-')[0];
}

function majorMinor(version) {
  const [major, minor] = core(version).split('.');
  return `${major}.${minor}`;
}

/** Compare two semver triples by their numeric core. */
function compare(a, b) {
  const left = core(a).split('.').map(Number);
  const right = core(b).split('.').map(Number);
  for (let i = 0; i < 3; i += 1) {
    if (left[i] !== right[i]) return left[i] < right[i] ? -1 : 1;
  }
  return 0;
}

/** Read a file at a git revision, or null when unavailable. */
function gitShow(ref, repoRelativePath) {
  try {
    return execFileSync('git', ['show', `${ref}:${repoRelativePath}`], {
      encoding: 'utf8',
      stdio: ['ignore', 'pipe', 'ignore'],
    });
  } catch {
    return null;
  }
}

function repoRelativePath(pkgPath) {
  try {
    const root = realpathSync(
      execFileSync('git', ['rev-parse', '--show-toplevel'], {
        encoding: 'utf8',
        stdio: ['ignore', 'pipe', 'ignore'],
      }).trim(),
    );
    // Resolve symlinks on both sides: /tmp → /private/tmp on macOS would
    // otherwise produce a path that git cannot resolve.
    return relative(root, realpathSync(pkgPath)).split(sep).join('/');
  } catch {
    return null;
  }
}

/**
 * The bykBinaryVersion of the previous committed state, or null when it cannot
 * be determined (no git, shallow clone, or the field did not exist yet).
 */
function previousBinaryVersion(pkgPath, current) {
  const relPath = repoRelativePath(pkgPath);
  if (!relPath) return null;

  const headRaw = gitShow('HEAD', relPath);
  const headBinary = headRaw ? JSON.parse(headRaw).bykBinaryVersion : undefined;

  // Working tree already committed (typical in CI) → compare against the parent.
  // Working tree ahead of HEAD → compare against HEAD.
  const ref = headBinary === current ? 'HEAD~1' : 'HEAD';
  const raw = ref === 'HEAD' ? headRaw : gitShow(ref, relPath);
  if (!raw) return null;

  return JSON.parse(raw).bykBinaryVersion ?? null;
}

function main() {
  const pkgPath = resolve(process.argv[2] || DEFAULT_PKG);
  let pkg;
  try {
    pkg = JSON.parse(readFileSync(pkgPath, 'utf8'));
  } catch (error) {
    fail(`cannot read ${pkgPath}: ${error.message}`);
  }

  const { version, bykBinaryVersion } = pkg;

  if (!version) fail('package.json has no "version"');
  if (!bykBinaryVersion) {
    fail('package.json has no "bykBinaryVersion" — it pins the bundled CLI release');
  }
  if (!SEMVER.test(version)) fail(`"version": "${version}" is not a semver triple`);
  if (!SEMVER.test(bykBinaryVersion)) {
    fail(`"bykBinaryVersion": "${bykBinaryVersion}" is not a semver triple`);
  }

  if (majorMinor(version) !== majorMinor(bykBinaryVersion)) {
    fail(
      `version ${version} and bykBinaryVersion ${bykBinaryVersion} must share major.minor\n` +
        `  the extension's X.Y is defined as the bundled CLI's major.minor;\n` +
        `  when the CLI moves to a new X.Y, bump the extension's X.Y too and reset its patch`,
    );
  }

  const previous = previousBinaryVersion(pkgPath, bykBinaryVersion);
  if (previous === null) {
    note('no previous bykBinaryVersion found — skipping monotonicity check');
  } else if (compare(bykBinaryVersion, previous) < 0) {
    if (process.env.BYK_ALLOW_BINARY_DOWNGRADE === '1') {
      note(`bykBinaryVersion ${previous} → ${bykBinaryVersion} (downgrade allowed)`);
    } else {
      fail(
        `bykBinaryVersion went backwards: ${previous} → ${bykBinaryVersion}\n` +
          `  a newer extension build must not ship an older CLI;\n` +
          `  set BYK_ALLOW_BINARY_DOWNGRADE=1 if this rollback is intentional`,
      );
    }
  }

  console.log(`✔ version ${version} bundles byk ${bykBinaryVersion}`);
}

main();
