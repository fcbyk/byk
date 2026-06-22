import * as vscode from 'vscode';
import * as path from 'path';
import * as fs from 'fs';

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
      // Object‑mode alias: { $cmd, $cwd?, $interactive? }
      const v = value as Record<string, unknown>;
      const cmd = typeof v.$cmd === 'string' ? v.$cmd : '';
      const parts = [cmd];
      if (v.$cwd) parts.push(`cwd:${v.$cwd}`);
      if (v.$interactive) parts.push('interactive');
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
): string | null {
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
  if (typeof value === 'string') return `@${stem}.${execPath}`;
  if (typeof value === 'object' && value !== null && !Array.isArray(value) && '$cmd' in value) return `@${stem}.${execPath}`;
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

// ── $cwd Path Completion ───────────────────────────────────────────

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

const bykCompletionProvider: vscode.CompletionItemProvider = {
  provideCompletionItems(document, position) {
    const cwd = isInCwdValue(document, position);
    if (!cwd) return undefined;

    const partialPath = cwd.line.substring(cwd.valueStart, position.character);

    // Resolve the directory to list
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
      // Relative to the .byk.json file's directory
      const fileDir = path.dirname(document.uri.fsPath);
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

      // Replace only the part after the last slash
      item.range = new vscode.Range(
        position.line,
        cwd.valueStart + partialPath.lastIndexOf('/') + 1,
        position.line,
        position.character,
      );
      item.insertText = entry.name + (entry.isDirectory() ? '/' : '');
      item.sortText = (entry.isDirectory() ? '0' : '1') + entry.name;

      items.push(item);
    }

    return items;
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

// ── Run Alias ──────────────────────────────────────────────────────

/** Extension path, set during activate. */
let extPath = '';

/** Expand ~ to the user's home directory. */
function resolveTilde(p: string): string {
  if (p.startsWith('~/') || p === '~') {
    return path.join(process.env.HOME || '/', p.length > 1 ? p.slice(2) : '');
  }
  return p;
}

/** Build the PATH for the shell execution based on current settings.
 *  Order: binaryPath dir → home dirs (if useSystemBinary) → system PATH → bundled dir
 */
function buildPath(): string {
  const parts: string[] = [];
  const home = process.env.HOME || '/';
  const binaryPath = vscode.workspace.getConfiguration('byk').get<string>('binaryPath', '');
  const useSystem = vscode.workspace.getConfiguration('byk').get<boolean>('useSystemBinary', false);

  // 1. Explicit binaryPath directory
  if (binaryPath) {
    const resolved = resolveTilde(binaryPath);
    try {
      fs.accessSync(resolved, fs.constants.X_OK);
    } catch {
      vscode.window.showErrorMessage(`byk binary not found at "${binaryPath}". Check your byk.binaryPath setting.`);
    }
    parts.push(path.dirname(resolved));
  }

  // 2. Home directories (useSystemBinary)
  if (useSystem) {
    parts.push(path.join(home, '.byk', 'bin'));
    parts.push(path.join(home, 'bin'));
    parts.push(path.join(home, '.local', 'bin'));
  }

  // 3. System PATH
  parts.push(process.env.PATH || '');

  // 4. Bundled binary directory (always last fallback)
  const platform = process.platform;
  const arch = process.arch;
  parts.push(path.join(extPath, 'bin', `${platform}-${arch}`));

  return parts.join(path.delimiter);
}

/** Run an alias as a VS Code Task — preserves task output formatting. */
function runAliasTask(exactCommand: string, cwd: string) {
  const options: vscode.ShellExecutionOptions = {
    cwd,
    env: { PATH: buildPath() },
  };

  const task = new vscode.Task(
    { type: 'byk' },
    vscode.TaskScope.Workspace,
    exactCommand,
    'byk',
    new vscode.ShellExecution(`byk ${exactCommand}`, options),
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

  // --- Commands ---

  // Right-click: precisely run the alias under the cursor (byk @stem.key)
  context.subscriptions.push(
    vscode.commands.registerCommand('byk-alias.runAtCursor', () => {
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

      const exactCommand = getAliasAtCursor(config, document, lineNumber, colNumber, stem);
      if (!exactCommand) {
        vscode.window.showInformationMessage('当前行没有可执行的别名');
        return;
      }

      runAliasTask(exactCommand, cwd);
    }),
  );

  // Right-click "Choose Alias…": show QuickPick with all aliases from this file
  context.subscriptions.push(
    vscode.commands.registerCommand('byk-alias.runScript', async (uri?: vscode.Uri) => {
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

export function deactivate() {}
