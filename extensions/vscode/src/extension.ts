import * as vscode from 'vscode';
import * as path from 'path';
import * as fs from 'fs';
import { spawn, spawnSync } from 'child_process';

// ── Helpers ────────────────────────────────────────────────────────

function isGroup(value: unknown): boolean {
  return typeof value === 'object' && value !== null && !Array.isArray(value) && !('$cmd' in value);
}

function resolvePath(obj: Record<string, unknown>, dotPath: string): unknown {
  let curr: unknown = obj;
  for (const part of dotPath.split('.')) {
    if (typeof curr !== 'object' || curr === null || Array.isArray(curr)) return undefined;
    curr = (curr as Record<string, unknown>)[part];
  }
  return curr;
}

/**
 * Parse a *.byk.json filename into the "file stem" used in the @ syntax.
 *   run.byk.json  → "run"
 *   .byk.json     → ""   (nameless form)
 */
function bykFileStem(filename: string): string {
  if (filename === '.byk.json') return '';
  // Strip the ".byk.json" suffix
  return filename.slice(0, filename.length - '.byk.json'.length);
}

/**
 * Return true when the filename matches the *.byk.json naming convention.
 * Nameless form (.byk.json) is also valid.
 * Names containing '.', '@', '/', ' ' are rejected (issue #3 spec).
 */
function isBykFile(filename: string): boolean {
  if (filename === '.byk.json') return true;
  if (!filename.endsWith('.byk.json')) return false;
  const stem = bykFileStem(filename);
  // Reject illegal characters in name
  if (/[.@/ ]/.test(stem)) return false;
  return stem.length > 0;
}

// ── Alias lookup ───────────────────────────────────────────────────

interface AliasItem {
  label: string;
  description: string;
  /** The exact `byk @stem.key` command to execute. */
  exactCommand: string;
}

/** A runnable alias found at the cursor, plus its raw config value. */
interface AliasHit {
  exactCommand: string;
  value: unknown;
}

/**
 * Flatten all runnable aliases in a single .byk.json file.
 * Skips all $‑prefixed system reserved fields ($priority, $cwd, etc.).
 * @param stem  The file stem (e.g. "run" for run.byk.json, "" for .byk.json)
 */
function flattenAliases(
  obj: Record<string, unknown>,
  stem: string,
  prefix = '',
): AliasItem[] {
  const items: AliasItem[] = [];
  for (const [key, value] of Object.entries(obj)) {
    // Skip system reserved fields (all $‑prefixed keys)
    if (key.startsWith('$')) continue;

    const fullPath = prefix ? `${prefix}.${key}` : key;

    if (typeof value === 'string') {
      items.push({
        label: `@${stem}.${fullPath}`,
        description: value,
        exactCommand: `@${stem}.${fullPath}`,
      });
    } else if (typeof value === 'object' && value !== null && !Array.isArray(value) && '$cmd' in value) {
      // Object‑mode alias: { $cmd, $cwd?, $mode?, $description? }
      const v = value as Record<string, unknown>;
      const cmd = typeof v.$cmd === 'string' ? v.$cmd : '';
      const desc = typeof v.$description === 'string' ? v.$description : '';
      const parts = desc ? [desc] : [cmd];
      if (v.$cwd) parts.push(`cwd:${v.$cwd}`);
      const mode = readMode(v.$mode);
      if (mode !== 'normal') parts.push(mode);
      items.push({
        label: `@${stem}.${fullPath}`,
        description: parts.join('  '),
        exactCommand: `@${stem}.${fullPath}`,
      });
    } else if (isGroup(value)) {
      items.push(...flattenAliases(value as Record<string, unknown>, stem, fullPath));
    }
  }

  return items;
}

/**
 * Find the alias at the cursor position. Reuses findKeyPositions
 * and reverse-looks up by line number.
 */
function getAliasAtCursor(
  config: Record<string, unknown>,
  document: vscode.TextDocument,
  lineNumber: number,
  colNumber: number,
  stem: string,
): AliasHit | null {
  // Collect all keys on the cursor line, pick the nearest one to the left
  let bestPath = '';
  let bestCol = -1;

  for (const [aliasPath, range] of findKeyPositions(document)) {
    if (range.start.line !== lineNumber) continue;
    if (range.start.character > colNumber) continue; // key starts after cursor
    if (range.start.character > bestCol) {
      bestCol = range.start.character;
      bestPath = aliasPath;
    }
  }

  if (!bestPath) return null;

  // Determine the executable alias path.
  // If cursor is on a system field (e.g. "build.$cmd"), resolve to parent.
  let execPath = bestPath;

  if (bestPath.startsWith('$')) return null; // root‑level system field

  if (bestPath.includes('.$')) {
    const parts = bestPath.split('.');
    while (parts.length > 0 && parts[parts.length - 1].startsWith('$')) {
      parts.pop();
    }
    if (parts.length === 0) return null;
    execPath = parts.join('.');
  }

  const value = resolvePath(config, execPath);
  if (typeof value === 'string') return { exactCommand: `@${stem}.${execPath}`, value };
  if (typeof value === 'object' && value !== null && !Array.isArray(value) && '$cmd' in value) return { exactCommand: `@${stem}.${execPath}`, value };
  return null; // group — not executable
}

// ── Key Position Map ───────────────────────────────────────────────
function findKeyPositions(document: vscode.TextDocument): Map<string, vscode.Range> {
  const map = new Map<string, vscode.Range>();
  const text = document.getText();
  // Strip JSON string content so braces inside strings don't confuse depth tracking
  const clean = text.replace(/"([^"\\]|\\.)*"/g, '""');

  // Count braces before each line to determine nesting depth
  const lines = text.split('\n');
  const cleanLines = clean.split('\n');
  const keyStack: string[] = [];
  let cleanHead = '';

  for (let i = 0; i < lines.length; i++) {
    const line = lines[i];
    const baseOpen = (cleanHead.match(/\{/g) || []).length;
    const baseClose = (cleanHead.match(/\}/g) || []).length;

    // Global match — one line may have multiple keys
    const keyRegex = /"([^"]+)"\s*:\s*/g;
    let keyMatch: RegExpExecArray | null;
    while ((keyMatch = keyRegex.exec(line)) !== null) {
      const key = keyMatch[1];
      const keyStart = keyMatch.index;
      const keyEnd = keyStart + key.length + 2; // +2 for the quotes

      // Count braces in the original line before this key (string‑aware),
      // so keys later on the same line see nesting from earlier keys/braces.
      let inlineOpen = 0, inlineClose = 0;
      let inString = false, esc = false;
      for (let c = 0; c < keyStart && c < line.length; c++) {
        const ch = line[c];
        if (esc) { esc = false; continue; }
        if (ch === '\\') { esc = true; continue; }
        if (ch === '"') { inString = !inString; continue; }
        if (!inString) {
          if (ch === '{') inlineOpen++;
          if (ch === '}') inlineClose++;
        }
      }

      const depth = Math.max(0, (baseOpen + inlineOpen) - (baseClose + inlineClose) - 1);

      keyStack.length = depth;
      keyStack.push(key);
      const fullPath = keyStack.join('.');

      map.set(fullPath, new vscode.Range(i, keyStart, i, keyEnd));
    }

    cleanHead += cleanLines[i] + '\n';
  }

  return map;
}

// ── $cwd / $paths Path Completion ──────────────────────────────────

/** Return whether the cursor sits inside a "$cwd" string value. */
function isInCwdValue(
  document: vscode.TextDocument,
  position: vscode.Position,
): { valueStart: number; line: string } | null {
  const line = document.lineAt(position.line).text;
  const m = line.match(/"\$cwd"\s*:\s*"/);
  if (!m || m.index === undefined) return null;
  const valueStart = m.index + m[0].length;
  if (position.character <= valueStart) return null;
  // Cursor must be before the closing quote
  const closeIdx = line.indexOf('"', valueStart);
  if (closeIdx !== -1 && position.character > closeIdx) return null;
  return { valueStart, line };
}

/** Return whether the cursor sits inside a "$paths" array string element. */
function isInPathsValue(
  document: vscode.TextDocument,
  position: vscode.Position,
): { valueStart: number; line: string } | null {
  const text = document.getText();
  const offset = document.offsetAt(position);

  // Find the last "$paths" before the cursor
  const pathsPattern = /"\$paths"\s*:\s*\[/g;
  let match: RegExpExecArray | null;
  let lastPathsEnd = -1;

  while ((match = pathsPattern.exec(text)) !== null) {
    if (match.index >= offset) break;
    lastPathsEnd = match.index + match[0].length;
  }

  if (lastPathsEnd === -1) return null;

  // Count bracket balance from after [ to cursor, skipping strings
  let bracketBalance = 0;
  let inString = false;
  let esc = false;
  for (let i = lastPathsEnd; i < offset; i++) {
    const ch = text[i];
    if (esc) { esc = false; continue; }
    if (ch === '\\') { esc = true; continue; }
    if (ch === '"') { inString = !inString; continue; }
    if (!inString) {
      if (ch === '[') bracketBalance++;
      if (ch === ']') bracketBalance--;
    }
  }

  // bracketBalance === 0 means we're still inside the array (no closing ] passed)
  if (bracketBalance !== 0) return null;

  // Check if cursor is inside a string value on the current line
  const line = document.lineAt(position.line).text;
  const strPattern = /"([^"\\]|\\.)*"/g;
  while ((match = strPattern.exec(line)) !== null) {
    const strStart = match.index;
    const strEnd = strStart + match[0].length;
    if (position.character > strStart && position.character < strEnd) {
      // Verify this string is after $paths[ in the document
      const lineStartOffset = document.offsetAt(new vscode.Position(position.line, 0));
      if (lineStartOffset + strStart >= lastPathsEnd) {
        return { valueStart: strStart + 1, line }; // +1 to skip opening quote
      }
    }
  }

  return null;
}

/** Resolve completion items for a partial path. */
function resolvePathCompletions(
  partialPath: string,
  fileDir: string,
  valueStart: number,
  position: vscode.Position,
): vscode.CompletionItem[] | undefined {
  let searchDir: string;

  if (partialPath.startsWith('/')) {
    const lastSlash = partialPath.lastIndexOf('/');
    searchDir = partialPath.substring(0, lastSlash + 1) || '/';
  } else if (partialPath.startsWith('~/')) {
    const home = process.env.HOME || '/';
    const rel = partialPath.slice(2);
    const lastSlash = rel.lastIndexOf('/');
    searchDir = path.join(home, lastSlash >= 0 ? rel.slice(0, lastSlash + 1) : '');
  } else {
    const resolved = path.join(fileDir, partialPath);
    const lastSlash = resolved.lastIndexOf('/');
    searchDir = resolved.slice(0, lastSlash + 1);
  }

  let entries: fs.Dirent[];
  try {
    entries = fs.readdirSync(searchDir, { withFileTypes: true });
  } catch {
    return undefined;
  }

  const prefix = partialPath.slice(partialPath.lastIndexOf('/') + 1);
  const items: vscode.CompletionItem[] = [];

  for (const entry of entries) {
    if (entry.name.startsWith('.') && prefix === '') continue;
    if (prefix && !entry.name.toLowerCase().startsWith(prefix.toLowerCase())) continue;

    const item = new vscode.CompletionItem(entry.name);
    item.kind = entry.isDirectory()
      ? vscode.CompletionItemKind.Folder
      : vscode.CompletionItemKind.File;

    item.range = new vscode.Range(
      position.line,
      valueStart + partialPath.lastIndexOf('/') + 1,
      position.line,
      position.character,
    );
    item.insertText = entry.name + (entry.isDirectory() ? '/' : '');
    item.sortText = (entry.isDirectory() ? '0' : '1') + entry.name;

    items.push(item);
  }

  return items;
}

const bykCompletionProvider: vscode.CompletionItemProvider = {
  provideCompletionItems(document, position) {
    const fileDir = path.dirname(document.uri.fsPath);

    // Check $cwd (single string value)
    const cwd = isInCwdValue(document, position);
    if (cwd) {
      const partialPath = cwd.line.substring(cwd.valueStart, position.character);
      return resolvePathCompletions(partialPath, fileDir, cwd.valueStart, position);
    }

    // Check $paths (array string element)
    const paths = isInPathsValue(document, position);
    if (paths) {
      const partialPath = paths.line.substring(paths.valueStart, position.character);
      return resolvePathCompletions(partialPath, fileDir, paths.valueStart, position);
    }

    return undefined;
  },
};

const bykTaskProvider: vscode.TaskProvider = {
  provideTasks(): vscode.ProviderResult<vscode.Task[]> {
    return []; // tasks are created via executeTask, not discovery
  },
  resolveTask(task: vscode.Task): vscode.ProviderResult<vscode.Task> {
    // Reconstruct with same definition — required by VS Code task resolution protocol
    return new vscode.Task(
      task.definition,
      task.scope ?? vscode.TaskScope.Workspace,
      task.name,
      task.source,
      task.execution,
    );
  },
};

// ── Binary resolution ──────────────────────────────────────────────
//
// One mechanism only: every candidate is a PATH entry, first match wins.
//
//   byk.priorityPaths    directories searched first, in array order
//   byk.secondarySource  which tier comes next — "bundled" (default) or
//                        "system"; the other one becomes the final fallback
//
// Directories that do not exist are skipped silently. A relative entry is
// rejected outright: PATH members resolve against the task's working
// directory, which the user cannot reason about from settings.json.
// Settings are read on every invocation, so edits apply without a reload.

/** Extension path, set during activate. */
let extPath = '';

const HOME_DIR = process.env.HOME || process.env.USERPROFILE || '/';
const IS_WINDOWS = process.platform === 'win32';
const BINARY_NAME = IS_WINDOWS ? 'byk.exe' : 'byk';

/** Expand a leading ~ to the user's home directory. */
function expandHome(p: string): string {
  if (p === '~') return HOME_DIR;
  if (p.startsWith('~/') || p.startsWith('~\\')) return path.join(HOME_DIR, p.slice(2));
  return p;
}

/** Expand ~ to the user's home directory (kept for $cwd resolution). */
function resolveTilde(p: string): string {
  return expandHome(p);
}

/** The name PATH is actually stored under — Windows uses `Path`, POSIX `PATH`. */
function pathEnvKey(): string {
  for (const key of Object.keys(process.env)) {
    if (key.toUpperCase() === 'PATH') return key;
  }
  return 'PATH';
}

/** Directory holding the binary shipped inside the extension package. */
function bundledDir(): string {
  return path.join(extPath, 'bin', `${process.platform}-${process.arch}`);
}

type BinarySource = 'bundled' | 'system';

interface BinarySettings {
  /** byk.priorityPaths with ~ expanded; entries that cannot be used land in `rejected`. */
  priorityPaths: string[];
  /** Entries this extension refuses to use, surfaced by the diagnostic report. */
  rejected: string[];
  source: BinarySource;
}

function binarySettings(): BinarySettings {
  const cfg = vscode.workspace.getConfiguration('byk');
  const raw = cfg.get<string[]>('priorityPaths', []) ?? [];
  const priorityPaths: string[] = [];
  const rejected: string[] = [];

  for (const entry of raw) {
    const trimmed = (entry ?? '').trim();
    if (!trimmed) continue;
    const expanded = expandHome(trimmed);
    if (path.isAbsolute(expanded)) priorityPaths.push(expanded);
    else rejected.push(trimmed);
  }

  return {
    priorityPaths,
    rejected,
    source: cfg.get<string>('secondarySource', 'bundled') === 'system' ? 'system' : 'bundled',
  };
}

function isDirectory(dir: string): boolean {
  try {
    return fs.statSync(dir).isDirectory();
  } catch {
    return false;
  }
}

/** process.env PATH as entries — empty segments are dropped (":" means cwd in POSIX). */
function systemDirs(): string[] {
  return (process.env[pathEnvKey()] || '')
    .split(path.delimiter)
    .map((dir) => dir.trim())
    .filter(Boolean);
}

/** Comparison key for a directory — case-insensitive on Windows. */
function dirKey(dir: string): string {
  const abs = path.resolve(dir);
  return IS_WINDOWS ? abs.toLowerCase() : abs;
}

function dedupeDirs(dirs: string[]): string[] {
  const seen = new Set<string>();
  const out: string[] = [];
  for (const dir of dirs) {
    if (!dir) continue;
    const key = dirKey(dir);
    if (seen.has(key)) continue;
    seen.add(key);
    out.push(dir);
  }
  return out;
}

/** Ordered search directories — index 0 has the highest priority. */
function binaryDirs(): string[] {
  const { priorityPaths, source } = binarySettings();
  const head = priorityPaths.filter(isDirectory);

  const bundled = bundledDir();
  const system = systemDirs();
  const tail = source === 'system' ? [...system, bundled] : [bundled, ...system];

  return dedupeDirs([...head, ...tail]);
}

/** PATH handed to every shell / child process. */
function buildPath(): string {
  return binaryDirs().join(path.delimiter);
}

// ── Alias File Diagnostics (Problems panel) ────────────────────────
//
// `byk __api__ check` scans the working directory and the global alias
// dir — the same two places the CLI reads — and returns scan problems
// as JSON. The scan is not recursive, so results are kept per directory
// and merged by file: two directories that both scan the global dir
// report the same items.

interface CheckItem {
  level: 'error' | 'warning';
  path: string;
  message: string;
}

interface CheckResponse {
  scanned: number;
  errors: number;
  warnings: number;
  items: CheckItem[];
}

/** Diagnostics per scanned directory. */
const checkByRoot = new Map<string, CheckItem[]>();
/** Every directory scanned at least once, so a binary change can re-run them. */
const checkRoots = new Set<string>();
/** Run generation per directory — a newer run supersedes an older one. */
const checkRuns = new Map<string, number>();
const checkTimers = new Map<string, NodeJS.Timeout>();

let diagnostics: vscode.DiagnosticCollection | undefined;

/** Call `byk __api__ check` in `root` and parse the JSON. Null on any failure. */
function callCheck(root: string): Promise<CheckResponse | null> {
  return new Promise((resolve) => {
    let settled = false;
    const finish = (value: CheckResponse | null) => {
      if (settled) return;
      settled = true;
      resolve(value);
    };

    let child: ReturnType<typeof spawn>;
    try {
      child = spawn('byk', ['__api__', 'check'], {
        cwd: root,
        env: { ...process.env, [pathEnvKey()]: buildPath() },
        windowsHide: true,
      });
    } catch {
      finish(null);
      return;
    }

    let stdout = '';
    const timer = setTimeout(() => child.kill(), 15000);
    child.stdout?.on('data', (chunk) => { stdout += chunk; });
    // A CLI without the `check` request exits 1 with a message on stderr;
    // this feature is absent, not broken, so stay silent.
    child.stderr?.resume();
    child.on('error', () => { clearTimeout(timer); finish(null); });
    child.on('close', (code) => {
      clearTimeout(timer);
      if (code !== 0) {
        finish(null);
        return;
      }
      try {
        const payload = JSON.parse(stdout) as CheckResponse;
        finish(Array.isArray(payload.items) ? payload : null);
      } catch {
        finish(null);
      }
    });
  });
}

function scheduleCheck(root: string, delay = 0): void {
  checkRoots.add(root);
  const pending = checkTimers.get(root);
  if (pending) clearTimeout(pending);
  checkTimers.set(root, setTimeout(() => {
    checkTimers.delete(root);
    void refreshCheck(root);
  }, delay));
}

async function refreshCheck(root: string): Promise<void> {
  const run = (checkRuns.get(root) ?? 0) + 1;
  checkRuns.set(root, run);

  const payload = await callCheck(root);
  if (checkRuns.get(root) !== run) return;

  if (payload === null) checkByRoot.delete(root);
  else checkByRoot.set(root, payload.items);
  renderDiagnostics();
}

/** Rebuild the whole collection; overlapping scans are deduped by file + message. */
function renderDiagnostics(): void {
  if (!diagnostics) return;

  const byFile = new Map<string, { uri: vscode.Uri; items: CheckItem[] }>();
  const seen = new Set<string>();

  for (const items of checkByRoot.values()) {
    for (const item of items) {
      const uri = vscode.Uri.file(item.path);
      const key = `${uri.toString()}\u0000${item.level}\u0000${item.message}`;
      if (seen.has(key)) continue;
      seen.add(key);

      let entry = byFile.get(uri.toString());
      if (!entry) {
        entry = { uri, items: [] };
        byFile.set(uri.toString(), entry);
      }
      entry.items.push(item);
    }
  }

  diagnostics.clear();
  for (const { uri, items } of byFile.values()) {
    diagnostics.set(
      uri,
      items.map((item) => {
        // `check` reports per file, not per line; point at the file start.
        const diagnostic = new vscode.Diagnostic(
          new vscode.Range(0, 0, 0, 0),
          item.message,
          item.level === 'error'
            ? vscode.DiagnosticSeverity.Error
            : vscode.DiagnosticSeverity.Warning,
        );
        diagnostic.source = 'byk';
        return diagnostic;
      }),
    );
  }
}

// ── Run Alias ──────────────────────────────────────────────────────

/** Run an alias as a VS Code Task — preserves task output formatting. */
function runAliasTask(exactCommand: string, cwd: string) {
  const options: vscode.ShellExecutionOptions = {
    cwd,
    env: { [pathEnvKey()]: buildPath() },
  };

  // PowerShell treats @ as the splat operator — escape every @ with a backtick.
  // cmd.exe, bash, zsh don't need this, and backtick would be literal there.
  const shellPath = vscode.env.shell || '';
  const isPowerShell = /powershell|pwsh/i.test(shellPath);
  const cmd = isPowerShell
    ? 'byk ' + exactCommand.replace(/@/g, '`@')
    : 'byk ' + exactCommand;

  const task = new vscode.Task(
    { type: 'byk' },
    vscode.TaskScope.Workspace,
    exactCommand,
    'byk',
    new vscode.ShellExecution(cmd, options),
  );
  task.presentationOptions = {
    echo: true,
    focus: true,
    reveal: vscode.TaskRevealKind.Always,
    panel: vscode.TaskPanelKind.Shared,
    showReuseMessage: true,
  };
  vscode.tasks.executeTask(task);
}

// ── Run Alias Directly (bypass byk) ───────────────────────────────

type AliasMode = 'normal' | 'interactive' | 'silent';

interface ResolveResult {
  key: string;
  source: string;
  sourcePath: string | null;
  command: string;
  cwd: string;
  paths: string[];
  mode?: AliasMode;
  /** Derived from `mode`; kept so a CLI that still only reports this stays safe. */
  interactive: boolean;
  description: string | null;
}

/**
 * Effective mode of a resolved alias.
 *
 * Prefers the authoritative `mode` field and falls back to the legacy
 * boolean — falling back the other way round would treat every alias as
 * `normal` and run interactive ones without their confirmation.
 */
function resolvedMode(resolved: ResolveResult): AliasMode {
  if (resolved.mode) return resolved.mode;
  return resolved.interactive ? 'interactive' : 'normal';
}

/** Call `byk __api__ resolve` and parse the JSON. Returns null on any failure. */
function callResolve(key: string, cwd: string): ResolveResult | null {
  let stdout: string;
  try {
    const result = spawnSync('byk', ['__api__', 'resolve', key], {
      cwd,
      env: { ...process.env, [pathEnvKey()]: buildPath() },
      encoding: 'utf-8',
      timeout: 15000,
      windowsHide: true,
    });
    if (result.error || result.status !== 0) return null;
    stdout = result.stdout;
  } catch {
    return null;
  }

  try {
    return JSON.parse(stdout) as ResolveResult;
  } catch {
    return null;
  }
}

/** Read `$mode` as a valid mode, ignoring anything unrecognised. */
function readMode(value: unknown): AliasMode {
  return value === 'interactive' || value === 'silent' || value === 'normal'
    ? value
    : 'normal';
}

/** Extract a runnable command (+ optional cwd/mode) from a raw config value. */
function rawValueToCommand(
  value: unknown,
): { command: string; cwd?: string; mode: AliasMode } | null {
  if (typeof value === 'string') {
    return value.trim() ? { command: value, mode: 'normal' } : null;
  }
  if (typeof value === 'object' && value !== null && !Array.isArray(value) && '$cmd' in value) {
    const v = value as Record<string, unknown>;
    if (typeof v.$cmd !== 'string' || !v.$cmd.trim()) return null;
    return {
      command: v.$cmd,
      cwd: typeof v.$cwd === 'string' && v.$cwd.trim() ? v.$cwd : undefined,
      mode: readMode(v.$mode),
    };
  }
  return null;
}

/** Execute a fully-resolved command as a VS Code task — no byk process involved. */
function executeDirectCommand(command: string, cwd: string, paths: string[], taskName: string) {
  let envPath = buildPath();
  if (paths.length > 0) {
    envPath = paths.join(path.delimiter) + path.delimiter + envPath;
  }

  const options: vscode.ShellExecutionOptions = {
    cwd,
    env: { [pathEnvKey()]: envPath },
  };

  // PowerShell treats @ as the splat operator — escape every @ with a backtick.
  const shellPath = vscode.env.shell || '';
  const isPowerShell = /powershell|pwsh/i.test(shellPath);
  const cmd = isPowerShell ? command.replace(/@/g, '`@') : command;

  const task = new vscode.Task(
    { type: 'byk' },
    vscode.TaskScope.Workspace,
    taskName,
    'byk',
    new vscode.ShellExecution(cmd, options),
  );
  task.presentationOptions = {
    echo: true,
    focus: true,
    reveal: vscode.TaskRevealKind.Always,
    panel: vscode.TaskPanelKind.Shared,
    showReuseMessage: true,
  };
  vscode.tasks.executeTask(task);
}

/**
 * Run the alias under the cursor WITHOUT going through byk for execution.
 * Calls `byk __api__ resolve` (read-only, exits immediately) to get the
 * resolved command, then executes it directly — so byk.exe is never resident
 * while the alias runs. This avoids the Windows file lock that blocks an
 * alias from rebuilding byk.exe itself.
 *
 * Interactive aliases fall back to `byk @key`; when the byk binary predates
 * the resolve endpoint, falls back to running the raw config value.
 */
function runAliasDirect(exactCommand: string, rawValue: unknown, configDir: string) {
  const resolved = callResolve(exactCommand, configDir);

  if (resolved) {
    if (resolvedMode(resolved) === 'interactive') {
      vscode.window.showInformationMessage(`"${exactCommand}" 需要交互输入，改用 byk 执行`);
      runAliasTask(exactCommand, configDir);
      return;
    }
    executeDirectCommand(
      resolved.command,
      resolved.cwd || configDir,
      resolved.paths ?? [],
      `${exactCommand} (direct)`,
    );
    return;
  }

  // resolve unavailable — fall back to the raw value from the config file.
  const raw = rawValueToCommand(rawValue);
  if (!raw) {
    vscode.window.showErrorMessage(`无法解析别名 "${exactCommand}"，且 byk 不支持 resolve`);
    return;
  }
  if (raw.mode === 'interactive') {
    runAliasTask(exactCommand, configDir);
    return;
  }

  vscode.window.showWarningMessage('byk resolve 不可用，直接运行原始命令（未做变量/占位符解析）');
  const rawCwd = raw.cwd ? path.resolve(configDir, resolveTilde(raw.cwd)) : configDir;
  executeDirectCommand(raw.command, rawCwd, [], `${exactCommand} (direct)`);
}

// ── Diagnostics ────────────────────────────────────────────────────

let outputChannel: vscode.OutputChannel | undefined;

function getOutputChannel(): vscode.OutputChannel {
  if (!outputChannel) outputChannel = vscode.window.createOutputChannel('BYK');
  return outputChannel;
}

/**
 * Print the resolved search order and the binary it lands on.
 * The only trustworthy answer to "which byk am I running" — a terminal's
 * `byk --version` reflects the terminal's PATH, not the one handed to tasks.
 */
function showBinaryReport(): void {
  const channel = getOutputChannel();
  const { priorityPaths, rejected, source } = binarySettings();
  const bundled = bundledDir();
  const priorityKeys = new Set(priorityPaths.filter(isDirectory).map(dirKey));

  channel.appendLine('BYK — binary resolution');
  channel.appendLine('');
  channel.appendLine(`  byk.secondarySource  ${source}`);
  channel.appendLine(`  platform             ${process.platform}-${process.arch}`);
  channel.appendLine('');
  channel.appendLine('  search order (top wins):');

  const entries = binaryDirs().map((dir) => ({
    dir,
    hasBinary: fs.existsSync(path.join(dir, BINARY_NAME)),
    tier: dirKey(dir) === dirKey(bundled)
      ? 'bundled '
      : priorityKeys.has(dirKey(dir))
        ? 'priority'
        : 'system  ',
  }));

  let resolved: string | null = null;
  let squashed = 0;

  const flushSquashed = () => {
    if (squashed === 0) return;
    channel.appendLine(`     … ${squashed} more PATH ${squashed === 1 ? 'directory' : 'directories'} without ${BINARY_NAME}`);
    squashed = 0;
  };

  for (const entry of entries) {
    if (entry.hasBinary && !resolved) resolved = path.join(entry.dir, BINARY_NAME);
    // PATH is long and mostly noise — only show the entries that can supply byk.
    if (entry.tier === 'system  ' && !entry.hasBinary) {
      squashed++;
      continue;
    }
    flushSquashed();
    channel.appendLine(`     [${entry.tier}] ${entry.dir}${entry.hasBinary ? '  ✓' : ''}`);
  }
  flushSquashed();

  if (rejected.length > 0) {
    channel.appendLine('');
    channel.appendLine('  ignored (not an absolute path):');
    for (const entry of rejected) channel.appendLine(`   - ${entry}`);
  }

  channel.appendLine('');
  if (resolved) {
    channel.appendLine(`  → ${resolved}`);
    const version = spawnSync(resolved, ['--version'], {
      encoding: 'utf-8',
      timeout: 5000,
      windowsHide: true,
    });
    const text = (version.stdout || version.stderr || '').trim();
    channel.appendLine(`    ${text || '(failed to run --version)'}`);
  } else {
    channel.appendLine(`  → no ${BINARY_NAME} found in any directory above`);
    channel.appendLine('    add the directory holding your byk to byk.priorityPaths');
  }

  channel.appendLine('');
  channel.show(true);
}

export function activate(context: vscode.ExtensionContext) {
  extPath = context.extensionPath;

  // Register task provider so VS Code recognizes the 'byk' task type
  context.subscriptions.push(
    vscode.tasks.registerTaskProvider('byk', bykTaskProvider),
  );

  // Register path completion for $cwd values
  context.subscriptions.push(
    vscode.languages.registerCompletionItemProvider(
      { pattern: '**/*.byk.json' },
      bykCompletionProvider,
      '/',
    ),
  );

  // --- Problems panel ---

  diagnostics = vscode.languages.createDiagnosticCollection('byk');
  context.subscriptions.push(diagnostics);

  const refreshDirOf = (uri: vscode.Uri, delay = 0) => {
    if (isBykFile(path.basename(uri.fsPath))) {
      scheduleCheck(path.dirname(uri.fsPath), delay);
    }
  };

  // The CLI scan is not recursive: cover the workspace roots and one
  // directory per alias file, or nested projects would stay unchecked
  // until their first save.
  const scanWorkspaceDirs = async () => {
    const roots = new Set<string>();
    for (const folder of vscode.workspace.workspaceFolders ?? []) {
      roots.add(folder.uri.fsPath);
    }
    try {
      const files = await vscode.workspace.findFiles('**/*.byk.json', '**/node_modules/**');
      for (const uri of files) roots.add(path.dirname(uri.fsPath));
    } catch {
      // No workspace: the folder roots above are all there is.
    }
    for (const root of roots) scheduleCheck(root);
  };
  void scanWorkspaceDirs();

  context.subscriptions.push(
    vscode.workspace.onDidSaveTextDocument((document) => refreshDirOf(document.uri)),
    vscode.workspace.onDidChangeWorkspaceFolders((event) => {
      for (const folder of event.removed) {
        const removed = folder.uri.fsPath;
        for (const root of [...checkRoots]) {
          if (root !== removed && !root.startsWith(removed + path.sep)) continue;
          const timer = checkTimers.get(root);
          if (timer) clearTimeout(timer);
          checkTimers.delete(root);
          checkRuns.delete(root);
          checkRoots.delete(root);
          checkByRoot.delete(root);
        }
      }
      renderDiagnostics();
      if (event.added.length > 0) void scanWorkspaceDirs();
    }),
    vscode.workspace.onDidChangeConfiguration((event) => {
      // priorityPaths / secondarySource can point at a different binary.
      if (!event.affectsConfiguration('byk')) return;
      checkByRoot.clear();
      renderDiagnostics();
      for (const root of checkRoots) scheduleCheck(root);
    }),
  );

  const bykFileWatcher = vscode.workspace.createFileSystemWatcher('**/*.byk.json');
  bykFileWatcher.onDidCreate((uri) => refreshDirOf(uri, 300));
  bykFileWatcher.onDidChange((uri) => refreshDirOf(uri, 300));
  bykFileWatcher.onDidDelete((uri) => refreshDirOf(uri, 300));
  context.subscriptions.push(bykFileWatcher);

  // --- Commands ---

  // Right-click: precisely run the alias under the cursor (byk @stem.key)
  context.subscriptions.push(
    vscode.commands.registerCommand('byk.runAtCursor', () => {
      const editor = vscode.window.activeTextEditor;
      if (!editor) return;

      const document = editor.document;
      const filename = path.basename(document.fileName);

      if (!isBykFile(filename)) {
        vscode.window.showErrorMessage(`Not a valid .byk.json file: ${filename}`);
        return;
      }

      const stem = bykFileStem(filename);
      const cwd = path.dirname(document.fileName);
      const lineNumber = editor.selection.active.line;
      const colNumber = editor.selection.active.character;

      let config: Record<string, unknown>;
      try {
        config = JSON.parse(document.getText());
      } catch {
        vscode.window.showErrorMessage(`Failed to parse ${filename}`);
        return;
      }

      const hit = getAliasAtCursor(config, document, lineNumber, colNumber, stem);
      if (!hit) {
        vscode.window.showInformationMessage('当前行没有可执行的别名');
        return;
      }

      runAliasTask(hit.exactCommand, cwd);
    }),
  );

  // Right-click: run the alias under the cursor directly (bypass byk execution)
  context.subscriptions.push(
    vscode.commands.registerCommand('byk.runDirect', () => {
      const editor = vscode.window.activeTextEditor;
      if (!editor) return;

      const document = editor.document;
      const filename = path.basename(document.fileName);

      if (!isBykFile(filename)) {
        vscode.window.showErrorMessage(`Not a valid .byk.json file: ${filename}`);
        return;
      }

      const stem = bykFileStem(filename);
      const cwd = path.dirname(document.fileName);
      const lineNumber = editor.selection.active.line;
      const colNumber = editor.selection.active.character;

      let config: Record<string, unknown>;
      try {
        config = JSON.parse(document.getText());
      } catch {
        vscode.window.showErrorMessage(`Failed to parse ${filename}`);
        return;
      }

      const hit = getAliasAtCursor(config, document, lineNumber, colNumber, stem);
      if (!hit) {
        vscode.window.showInformationMessage('当前行没有可执行的别名');
        return;
      }

      runAliasDirect(hit.exactCommand, hit.value, cwd);
    }),
  );

  // Command palette: report which byk binary the extension actually resolves
  context.subscriptions.push(
    vscode.commands.registerCommand('byk.showBinary', () => {
      showBinaryReport();
    }),
  );

  // Right-click "Choose Alias…": show QuickPick with all aliases from this file
  context.subscriptions.push(
    vscode.commands.registerCommand('byk.runScript', async (uri?: vscode.Uri) => {
      let fileUri = uri;
      if (!fileUri) {
        const editor = vscode.window.activeTextEditor;
        if (editor && isBykFile(path.basename(editor.document.fileName))) {
          fileUri = editor.document.uri;
        }
      }
      if (!fileUri) {
        vscode.window.showErrorMessage('No .byk.json file found');
        return;
      }

      const filename = path.basename(fileUri.fsPath);
      if (!isBykFile(filename)) {
        vscode.window.showErrorMessage(`Not a valid .byk.json file: ${filename}`);
        return;
      }

      const stem = bykFileStem(filename);

      let config: Record<string, unknown>;
      try {
        config = JSON.parse(fs.readFileSync(fileUri.fsPath, 'utf-8'));
      } catch (err) {
        vscode.window.showErrorMessage(`Failed to parse ${filename}: ${(err as Error).message}`);
        return;
      }

      const aliases = flattenAliases(config, stem);
      if (aliases.length === 0) {
        vscode.window.showInformationMessage('No aliases found');
        return;
      }

      const picker = vscode.window.createQuickPick<vscode.QuickPickItem & { exactCommand: string }>();
      picker.placeholder = 'Search or select an alias to run…';
      picker.matchOnDescription = true;
      picker.items = aliases.map((a) => ({
        label: a.label,
        description: a.description,
        exactCommand: a.exactCommand,
      }));
      picker.show();

      const selected = await new Promise<(vscode.QuickPickItem & { exactCommand: string }) | undefined>((resolve) => {
        picker.onDidAccept(() => resolve(picker.selectedItems[0] as vscode.QuickPickItem & { exactCommand: string }));
        picker.onDidHide(() => resolve(undefined));
      });
      picker.dispose();

      if (!selected) return;

      runAliasTask(selected.exactCommand, path.dirname(fileUri.fsPath));
    }),
  );
}

export function deactivate() {
  for (const timer of checkTimers.values()) clearTimeout(timer);
  checkTimers.clear();
}