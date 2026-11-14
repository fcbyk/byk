#!/usr/bin/env python3
"""发布脚本：选版本 → 归档 changelog → 确认扩展版本 → 跑全量测试 → 写盘 → 提交并打标签。

写盘是倒数第二步、提交是最后一步，前面几步都不动文件；测试也放在写盘之前，
失败时仓库一个字节都没改，不需要为半成品写回滚。

几条不写在代码里就会忘的约定：

  - 版本唯一真值是 Cargo.toml（release.yml 会校验 tag 与它一致，docs 构建也从
    这里读）。Cargo.lock 交给 `cargo update -p byk` 自己改，脚本不碰它的内容。
  - changelog 只做标题重命名，不生成条目 —— 代写出来的句子没人认领。
  - VS Code 扩展的 version / bykBinaryVersion 发版前单独询问：扩展 X.Y 跟随
    CLI、Z 独立，跨 X.Y 时 Z 重置为 0。
  - 提交不筛选文件：发版时工作区本来就有手写的 changelog，只报数量不列名字。
  - 不 push：推 tag 会直接触发 release 构建，那一步留给人。

用法：python3 scripts/release.py
"""

from __future__ import annotations

import os
import re
import shutil
import subprocess
import sys
import time
import unicodedata
from collections.abc import Callable
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
MANIFEST = ROOT / "Cargo.toml"
LOCK = ROOT / "Cargo.lock"
VSCODE_PKG = ROOT / "extensions" / "vscode" / "package.json"
RULE = "------------------"
SEMVER = re.compile(r"^(\d+)\.(\d+)\.(\d+)(?:-[0-9A-Za-z.-]+)?$")

# 两份 changelog 的待发布标题不是同一个词，中文那份用「未发布」，直接沿用。
# breaking 只用来在选 patch 时提个醒。
CHANGELOGS = [
    {"name": "CHANGELOG.md", "pending": "Unreleased", "breaking": "Breaking Changes"},
    {"name": "CHANGELOG.zh.md", "pending": "未发布", "breaking": "破坏性变更"},
]
for _entry in CHANGELOGS:
    _entry["path"] = ROOT / _entry["name"]


class ReleaseError(Exception):
    """脚本主动中止：消息直接打给作者看。"""


# ── 输出 ────────────────────────────────────────────────────────────

# 只有交互终端才上色：管道里跑（脚本自测）输出保持干净
COLOR = sys.stdout.isatty() and not os.environ.get("NO_COLOR")
TTY = sys.stdout.isatty()


def paint(code: str) -> Callable[[str], str]:
    def wrap(text: str) -> str:
        return f"\x1b[{code}m{text}\x1b[0m" if COLOR else text

    return wrap


bold = paint("1")
dim = paint("2")
# 提示符用黄色：跳出来但不像红/绿那样带成功失败的语义
accent = paint("33")
green = paint("32")
yellow = paint("33")
red = paint("31")


def cancelled() -> None:
    print(yellow("已取消，未改动任何文件。"))
    sys.exit(1)


def ask(prompt: str) -> str:
    """手动 readline 而不是 input()：EOF 和管道下的回显都要自己控制。"""
    sys.stdout.write(prompt)
    sys.stdout.flush()
    line = sys.stdin.readline()
    if line == "":
        cancelled()
    answer = line.rstrip("\r\n")
    if not sys.stdin.isatty():
        print(answer)
    return answer


# ── 版本号 ──────────────────────────────────────────────────────────


def locate_version(text: str) -> tuple[int, str, str]:
    """只认 [package] 段里行首的 version，[dependencies] 的内联表不会误命中。"""
    section = ""
    for index, line in enumerate(text.split("\n")):
        trimmed = line.strip()
        if trimmed.startswith("["):
            section = trimmed
            continue
        if section != "[package]":
            continue

        matched = re.match(r'^\s*version\s*=\s*"([^"]+)"', line)
        if matched:
            return index, line, matched.group(1)

    raise ReleaseError("Cargo.toml 里找不到 [package] 段的 version，先手动确认文件结构")


def parse_version(version: str) -> tuple[int, int, int] | None:
    matched = SEMVER.match(version)
    return tuple(int(part) for part in matched.groups()) if matched else None


def bump(version: str, kind: str) -> str:
    parts = parse_version(version)
    if parts is None:
        raise ReleaseError(f"当前版本不是合法 semver: {version}")

    major, minor, patch = parts
    if kind == "patch":
        return f"{major}.{minor}.{patch + 1}"
    if kind == "minor":
        return f"{major}.{minor + 1}.0"
    return f"{major + 1}.0.0"


def compare(left_version: str, right_version: str) -> int:
    """逐段比较，避开 '0.10.0' < '0.9.0' 这种字符串比较的坑。"""
    left = parse_version(left_version)
    right = parse_version(right_version)
    if left is None or right is None:
        return 0

    for a, b in zip(left, right):
        if a != b:
            return -1 if a < b else 1
    return 0


def set_version(line: str, next_version: str) -> str:
    """只换那一行里的版本字符串，缩进和行尾内容保持原样。"""
    return re.sub(r'version\s*=\s*"[^"]+"', f'version = "{next_version}"', line, count=1)


def rewrite(text: str, index: int, next_version: str) -> str:
    lines = text.split("\n")
    lines[index] = set_version(lines[index], next_version)
    return "\n".join(lines)


def json_field(text: str, key: str, name: str) -> re.Match[str]:
    """定位顶层 JSON 字符串字段；同名键必须恰好出现一次。"""
    pattern = rf'^(\s*"{re.escape(key)}":\s*")([^"]+)(")'
    matches = list(re.finditer(pattern, text, re.MULTILINE))
    if len(matches) != 1:
        raise ReleaseError(
            f"{name} 里应恰有一个 \"{key}\" 字段，实际找到 {len(matches)} 个"
        )
    return matches[0]


def json_value(text: str, key: str, name: str) -> str:
    return json_field(text, key, name).group(2)


def plan_json_field(text: str, key: str, next_version: str, name: str) -> dict | None:
    """算出单个字段的改写位置与前后整行；值已一致时返回 None。

    只替换值本身，缩进、逗号和其余字段保持原样，避免整份 JSON 重新序列化。
    """
    match = json_field(text, key, name)
    current = match.group(2)
    if current == next_version:
        return None

    start, end = match.start(2), match.end(2)
    line_start = text.rfind("\n", 0, start) + 1
    line_end = text.find("\n", end)
    if line_end < 0:
        line_end = len(text)
    before = text[line_start:line_end]
    after = before[: start - line_start] + next_version + before[end - line_start :]
    return {
        "key": key,
        "current": current,
        "next": next_version,
        "start": start,
        "end": end,
        "line": text.count("\n", 0, start) + 1,
        "before": before.strip(),
        "after": after.strip(),
    }


def pick_version(current: str, suggested: list[str]) -> str:
    """空回车按 1（patch）：大多数发布都是 patch。"""
    while True:
        picked = ask(f"{accent('选择:')} ").strip()

        if picked in ("", "1", "patch"):
            return suggested[0]
        if picked in ("2", "minor"):
            return suggested[1]
        if picked in ("3", "major"):
            return suggested[2]
        if picked in ("4", "input", "custom"):
            return ask_version(current)

        print(red(f"无法识别的选择: {picked}"))


def ask_version(current: str) -> str:
    while True:
        typed = ask(f"{accent('输入版本:')} ").strip().removeprefix("v")

        if parse_version(typed) is None:
            print(red(f"版本号不合法: {typed or '(空)'}，需要 X.Y.Z"))
            continue
        if compare(typed, current) <= 0:
            print(red(f"{typed} 不大于当前版本 {current}"))
            continue
        return typed


# ── changelog ───────────────────────────────────────────────────────


def today() -> str:
    return time.strftime("%Y-%m-%d")


def display_width(text: str) -> int:
    """CJK 算两列：`## Unreleased` 与 `## 未发布` 宽度差一倍，不对齐箭头会错开。"""
    return sum(2 if unicodedata.east_asian_width(ch) in "WF" else 1 for ch in text)


def find_heading(text: str, pattern: str) -> tuple[int, str, list[str]] | None:
    lines = text.split("\n")
    matcher = re.compile(pattern)
    for index, line in enumerate(lines):
        if matcher.match(line):
            return index, line, lines
    return None


def count_entries(lines: list[str], start: int) -> int:
    """只数 `- ` 开头的行，遇到下一个二级标题就停。"""
    total = 0
    for line in lines[start + 1 :]:
        if re.match(r"^##\s", line):
            break
        if re.match(r"^-\s", line):
            total += 1
    return total


def has_section(lines: list[str], start: int, title: str) -> bool:
    matcher = re.compile(rf"^###\s+{re.escape(title)}(\s|$)")
    for line in lines[start + 1 :]:
        if re.match(r"^##\s", line):
            return False
        if matcher.match(line):
            return True
    return False


def inspect_changelog(entry: dict, target: str) -> dict:
    """判这一版能不能归档：done / pending / manual / missing。

    标题匹配带边界：`0.8.0` 不能匹配到 `0.8.01`，也不能匹配到 `0.8.0-rc.1`。
    """
    path = entry["path"]
    if not path.exists():
        return {"file": entry, "state": "missing"}

    text = path.read_text(encoding="utf-8")
    if find_heading(text, rf"^##\s+v?{re.escape(target)}(\s|$)"):
        return {"file": entry, "state": "done"}

    pending = find_heading(text, rf"^##\s+{re.escape(entry['pending'])}(\s|$)")
    if pending is None:
        return {"file": entry, "state": "manual"}

    index, line, lines = pending
    return {
        "file": entry,
        "state": "pending",
        "text": text,
        "index": index,
        "before": line.strip(),
        "entries": count_entries(lines, index),
        "breaking": has_section(lines, index, entry["breaking"]),
    }


def is_patch_level(target: str, current: str) -> bool:
    """同一个 X.Y 就是 patch 级：这种情况下的破坏性变更多半是升错了。"""
    nxt = parse_version(target)
    prev = parse_version(current)
    return nxt is not None and prev is not None and nxt[:2] == prev[:2]


def pick_changelog_action(can_replace: bool) -> str:
    """编号固定不变：菜单在不同状态下长得不一样，编号跟着变会让人按错。

    没有待发布段时 1 不可用，此时空回车落到 2，免得回车只换来一句报错。
    """
    while True:
        picked = ask(f"{accent('选择:')} ").strip()

        if picked == "":
            return "replace" if can_replace else "check"
        if picked == "1":
            if can_replace:
                return "replace"
            print(red("没有可替换的待发布段，先把标题补好再按 1"))
            continue
        if picked == "2":
            return "check"
        if picked == "3":
            return "skip"
        print(red(f"无法识别的选择: {picked}"))


def print_todo(report: dict, heading: str) -> None:
    entry = report["file"]
    print(f"{entry['name']}  需要手动处理")
    print(dim(f"  在文件顶部加上 {heading}"))
    print(dim(f"  或者补一个 ## {entry['pending']} 段，下次运行再让脚本改名"))


def resolve_changelogs(target: str, current: str) -> list[dict]:
    """changelog 闸门：全部通过才返回待写入的改动（不含写盘）。

      1) 替换 unreleased   把待发布段的标题改成这一版
      2) 检查 changelog    立刻重扫一遍，报出还缺什么，然后回到菜单
      3) 跳过              这次不归档，直接往下走

    两份都就绪时不弹菜单，只打一行 ✓；「检查」不等回车，人改完再按 2 即可。
    """
    heading = f"## v{target} ({today()})"
    checked = False

    while True:
        reports = [inspect_changelog(entry, target) for entry in CHANGELOGS]
        for report in reports:
            if report["state"] == "missing":
                print(yellow(f"{report['file']['name']} 不存在，跳过"))

        pending = [report for report in reports if report["state"] == "pending"]
        manual = [report for report in reports if report["state"] == "manual"]
        ready = [report for report in reports if report["state"] == "done"]

        if not pending and not manual:
            # 什么都不打会让人怀疑「它到底查了没有」
            if ready:
                print(dim(RULE))
                print("changelog")
                names = "、".join(report["file"]["name"] for report in ready)
                print(green(f"✓ 已有 {heading} 的记录：{names}"))
            return []

        # 有待手动补的，就没有整段可替换的对象
        can_replace = not manual

        print(dim(RULE))
        print("changelog")

        if checked:
            # 表格没变化，重画一遍看着像卡住了
            unfinished = "、".join(report["file"]["name"] for report in [*pending, *manual])
            print(red(f"✖ 还没有 {heading} 的记录：{unfinished}"))
            print(dim("  改好后再按 2 重新检查"))
        else:
            if pending:
                name_width = max(len(report["file"]["name"]) for report in pending)
                before_width = max(display_width(report["before"]) for report in pending)
                for report in pending:
                    count = report["entries"]
                    note = "空" if count == 0 else f"{count} 条"
                    pad = " " * (before_width - display_width(report["before"]))
                    print(
                        f"{report['file']['name'].ljust(name_width)}  "
                        f"{report['before']}{pad} → {heading}  {dim(note)}"
                    )
                if any(report["entries"] == 0 for report in pending):
                    print(yellow("  待发布段是空的，这一版不会留下任何条目"))
                if any(report["breaking"] for report in pending) and is_patch_level(target, current):
                    print(yellow("  待发布段里有破坏性变更，通常该选 minor 或 major"))

            for report in manual:
                print_todo(report, heading)

        print(dim(RULE))
        if can_replace:
            print("1) 替换 unreleased")
        else:
            print(f"1) 替换 unreleased  {dim('（没有待发布段）')}")
        print("2) 检查 changelog")
        print("3) 跳过")

        action = pick_changelog_action(can_replace)

        if action == "skip":
            print(dim("已跳过 changelog，这一版不会有归档记录"))
            return []

        if action == "replace":
            return [
                {
                    "path": report["file"]["path"],
                    "name": report["file"]["name"],
                    "text": report["text"],
                    "index": report["index"],
                    "before": report["before"],
                    "after": heading,
                }
                for report in pending
            ]

        checked = True


# ── VS Code 扩展版本 ────────────────────────────────────────────────


def resolve_vscode(target: str) -> list[dict]:
    """扩展版本闸门：询问是否把扩展与内置 CLI 的版本升到这一版。

    - `bykBinaryVersion` 必须指向本次发版的 CLI，否则扩展打包时会内置旧二进制。
    - 扩展的 `version` 的 X.Y 跟随 CLI；同一 X.Y 内 Z 独立前进，跨 X.Y 时重置为 0。
    - 只替换字段值，不重新序列化整份 JSON。
    """
    name = "extensions/vscode/package.json"

    print(dim(RULE))
    print("vscode")
    if not VSCODE_PKG.exists():
        print(yellow(f"  {name} 不存在，跳过"))
        return []

    text = VSCODE_PKG.read_text(encoding="utf-8")
    ext_now = json_value(text, "version", name)
    bin_now = json_value(text, "bykBinaryVersion", name)
    ext = parse_version(ext_now)
    nxt = parse_version(target)
    if ext is None:
        raise ReleaseError(f"{name}: version {ext_now!r} 不是合法 semver")
    if parse_version(bin_now) is None:
        raise ReleaseError(f"{name}: bykBinaryVersion {bin_now!r} 不是合法 semver")
    if nxt is None:
        raise ReleaseError(f"目标版本不是合法 semver: {target}")

    if ext[:2] != nxt[:2]:
        if ext[:2] > nxt[:2]:
            raise ReleaseError(
                f"{name}: 扩展版本 {ext_now} 高于本次 CLI {target}，先确认要发的版本"
            )
        ext_next = f"{nxt[0]}.{nxt[1]}.0"
    else:
        ext_next = ext_now if compare(ext_now, target) >= 0 else target

    if compare(bin_now, target) > 0:
        raise ReleaseError(
            f"{name}: bykBinaryVersion {bin_now} 已高于本次 CLI {target}，先确认要发的版本"
        )
    bin_next = target

    edits = [
        edit
        for edit in (
            plan_json_field(text, "version", ext_next, name),
            plan_json_field(text, "bykBinaryVersion", bin_next, name),
        )
        if edit
    ]
    if not edits:
        print(green(f"✓ {name} 已是 {ext_next} / CLI {bin_next}"))
        return []

    print(name)
    for edit in edits:
        print(f"  {edit['key'].ljust(16)} {edit['current']} → {edit['next']}")

    if not confirm("更新 VS Code 扩展版本? [y/n]"):
        print(yellow("已跳过；扩展内置的 CLI 可能与本次发版不匹配"))
        return []
    return [{"path": VSCODE_PKG, "name": name, "text": text, "edits": edits}]


# ── 测试 ────────────────────────────────────────────────────────────

LABEL_WIDTH = 26


def human_time(seconds: float) -> str:
    if seconds < 60:
        return f"{seconds:.1f}s"
    return f"{int(seconds // 60)}m{int(seconds % 60):02d}s"


def parse_cargo(output: str) -> tuple[int, int]:
    """cargo test 每个 target 打一行 test result，全部累加。"""
    passed = failed = 0
    for matched in re.finditer(r"test result: \w+\. (\d+) passed; (\d+) failed", output):
        passed += int(matched.group(1))
        failed += int(matched.group(2))
    return passed, failed


def parse_pytest(output: str) -> tuple[int, int]:
    passed = re.search(r"(\d+) passed", output)
    failed = re.search(r"(\d+) failed", output)
    return (
        int(passed.group(1)) if passed else 0,
        int(failed.group(1)) if failed else 0,
    )


def summarize(ok: bool, parser: Callable[[str], tuple[int, int]] | None, output: str) -> str:
    if parser is None:
        return "ok" if ok else "失败"

    passed, failed = parser(output)
    if failed:
        return f"{failed} failed"
    if not ok:
        return "失败"
    return f"{passed} passed"


def run_step(
    label: str, cmd: list[str], parser: Callable[[str], tuple[int, int]] | None
) -> tuple[bool, str, str, float]:
    started = time.monotonic()
    if TTY:
        sys.stdout.write(f"{label.ljust(LABEL_WIDTH)}  {dim('running…')}")
        sys.stdout.flush()

    try:
        proc = subprocess.run(cmd, cwd=ROOT, capture_output=True, text=True)
        output = (proc.stdout or "") + (proc.stderr or "")
        ok = proc.returncode == 0
    except FileNotFoundError:
        output = f"找不到命令: {cmd[0]}"
        ok = False

    elapsed = time.monotonic() - started
    status = summarize(ok, parser, output)

    # 用回车加清行把 running… 换成结果，省得滚一屏
    if TTY:
        sys.stdout.write("\r\x1b[K")
    colored = green(status.ljust(12)) if ok else red(status.ljust(12))
    print(f"{label.ljust(LABEL_WIDTH)}{colored} {dim(human_time(elapsed))}")

    return ok, status, output, elapsed


def pytest_command() -> list[str] | None:
    """e2e 依赖 pty，Windows 上没有（CI 也只在 ubuntu 跑）。"""
    if os.name == "nt":
        return None
    if shutil.which("pytest"):
        return ["pytest", "-q"]
    return [sys.executable, "-m", "pytest", "-q"]


def last_lines(output: str, count: int) -> list[str]:
    lines = [line.rstrip() for line in output.strip().splitlines() if line.strip()]
    return lines[-count:]


def run_tests() -> None:
    """全量跑，不给菜单：挑着跑的测试闸门等于没有闸门。

    先 cargo build 再 pytest，和 CI 同一个理由：conftest 的 fixture 自己也会
    编译，先编一次能让编译错误不伪装成测试失败。
    """
    steps: list[tuple[str, list[str], Callable[[str], tuple[int, int]] | None]] = [
        ("cargo build", ["cargo", "build"], None),
        ("cargo test --all-targets", ["cargo", "test", "--all-targets"], parse_cargo),
    ]
    e2e = pytest_command()
    if e2e is not None:
        steps.append(("pytest -q", e2e, parse_pytest))

    print(dim(RULE))
    print("test")
    if e2e is None:
        print(yellow("  e2e 依赖 pty，Windows 上跳过"))
    if not (ROOT / "target" / "debug").exists():
        print(dim("  target/ 是空的，第一次构建可能要几分钟"))

    total = 0.0
    for label, cmd, parser in steps:
        ok, _status, output, elapsed = run_step(label, cmd, parser)
        total += elapsed
        if not ok:
            print(dim(RULE))
            print(red(f"✖ {label} 失败，未改动任何文件"))
            for line in last_lines(output, 12):
                print(dim(f"  {line}"))
            sys.exit(1)

    print(dim(RULE))
    print(green(f"✓ 测试通过 {human_time(total)}"))


# ── 写盘 ────────────────────────────────────────────────────────────


def sync_lock() -> None:
    """Cargo.lock 交给 cargo 自己改，脚本不碰它的内容。

    `-p byk` 只动本地包那一条；`--offline` 免掉一次 registry 索引刷新。失败只
    负责抛错，回滚由调用方统一做 —— 那时它知道本次一共改过哪些文件。
    """
    try:
        proc = subprocess.run(
            ["cargo", "update", "-p", "byk", "--offline"],
            cwd=ROOT,
            capture_output=True,
            text=True,
        )
    except FileNotFoundError:
        raise ReleaseError("找不到 cargo（PATH 里没有）—— 确认 Rust 装好再跑一次")

    if proc.returncode == 0:
        return

    detail = "\n  ".join((proc.stderr or "").strip().splitlines()[:3])
    message = ["cargo update -p byk --offline 失败"]
    if detail:
        message.append(f"  {detail}")
    message.append("  手动跑一次 cargo update -p byk 看看具体原因")
    raise ReleaseError("\n".join(message))


# ── 提交 ────────────────────────────────────────────────────────────


def git(*args: str) -> subprocess.CompletedProcess[str]:
    return subprocess.run(["git", *args], cwd=ROOT, capture_output=True, text=True)


def head_lines(output: str, count: int = 3) -> str:
    lines = [line.rstrip() for line in output.strip().splitlines()[:count]]
    return "\n  ".join(line for line in lines if line)


def changed_files() -> int:
    """含未跟踪文件。"""
    proc = git("status", "--porcelain")
    return sum(1 for line in proc.stdout.splitlines() if line.strip())


def tag_exists(tag: str) -> bool:
    return git("rev-parse", "--verify", "--quiet", f"refs/tags/{tag}").returncode == 0


def confirm(prompt: str) -> bool:
    """只认 y / n —— 唯一退不回来的一步，不该存在「手快回车」的路径。"""
    while True:
        answer = ask(f"{accent(prompt)} ").strip().lower()
        if answer == "y":
            return True
        if answer == "n":
            return False
        print(red("输入有误，只接受 y 或 n"))


def commit_and_tag(version: str) -> str:
    """提交工作区并打标签，返回短 sha。

    沿用仓库既有约定：消息 `release: vX.Y.Z`，标签是轻量的 `vX.Y.Z`（v0.7.0
    的 objecttype 就是 commit）。commit 成功、tag 失败要分开报：那时提交已经
    在历史里了，谎报「全都失败」会让人以为可以重跑。
    """
    staged = git("add", "-A")
    if staged.returncode != 0:
        raise ReleaseError(f"git add 失败\n  {head_lines(staged.stderr)}")

    commit = git("commit", "-m", f"release: v{version}")
    if commit.returncode != 0:
        raise ReleaseError(
            "git commit 失败，改动仍在工作区（已 add）\n  "
            + head_lines(commit.stdout + commit.stderr)
        )

    tag = git("tag", f"v{version}")
    if tag.returncode != 0:
        raise ReleaseError(
            "提交已完成，但打 tag 失败\n  "
            + head_lines(tag.stderr)
            + f"\n  补救：git tag v{version}"
        )

    return git("rev-parse", "--short", "HEAD").stdout.strip()


def main() -> None:
    text = MANIFEST.read_text(encoding="utf-8")
    index, line, current = locate_version(text)
    has_lock = LOCK.exists()
    try:
        shown = str(MANIFEST.relative_to(Path.cwd()))
    except ValueError:
        shown = str(MANIFEST)

    print()
    print(f"manifest  {shown}")
    print(f"current   {bold(current)}")
    print(dim(RULE))

    kinds = ["patch", "minor", "major"]
    suggested = [bump(current, kind) for kind in kinds]
    for position, kind in enumerate(kinds, start=1):
        print(f"{position}) {kind} {suggested[position - 1]}")
    print("4) input")

    target = pick_version(current, suggested)

    # 标签已存在就别往下跑：后面要花时间和交互，最后才发现发不出去最亏
    if tag_exists(f"v{target}"):
        raise ReleaseError(f"tag v{target} 已存在，先确认这次要发的版本号")

    # 所有闸门都在写盘之前：不过就一直问，宁可什么都没改
    archived = resolve_changelogs(target, current)
    vscode = resolve_vscode(target)
    run_tests()

    manifest_before = line.strip()
    manifest_after = set_version(manifest_before, target)

    print(dim(RULE))
    print(f"{shown}:{index + 1}")
    print(red(f"- {manifest_before}"))
    print(green(f"+ {manifest_after}"))
    if has_lock:
        print(dim("Cargo.lock   由 cargo update -p byk 同步"))
    for edit in archived:
        print(f"{edit['name']}:{edit['index'] + 1}")
        print(red(f"- {edit['before']}"))
        print(green(f"+ {edit['after']}"))
    for edit in vscode:
        for field in edit["edits"]:
            print(f"{edit['name']}:{field['line']}")
            print(red(f"- {field['before']}"))
            print(green(f"+ {field['after']}"))
    print(dim(RULE))

    # 记下每个文件的原内容：任何一步失败都整体还原，不留半成品
    originals: dict[Path, str] = {}

    def write(path: Path, content: str) -> None:
        originals.setdefault(path, path.read_text(encoding="utf-8"))
        path.write_text(content, encoding="utf-8")

    try:
        write(MANIFEST, rewrite(text, index, target))
        if has_lock:
            sync_lock()
        for edit in archived:
            lines = edit["text"].split("\n")
            lines[edit["index"]] = edit["after"]
            write(Path(edit["path"]), "\n".join(lines))
        for edit in vscode:
            content = edit["text"]
            for field in sorted(edit["edits"], key=lambda f: f["start"], reverse=True):
                content = content[: field["start"]] + field["next"] + content[field["end"] :]
            write(edit["path"], content)
    except (ReleaseError, OSError) as error:
        for path, content in originals.items():
            path.write_text(content, encoding="utf-8")
        raise ReleaseError(f"{error}\n已回滚 {len(originals)} 个文件，未留下半成品") from error

    done = [f"Cargo.toml{' 与 Cargo.lock' if has_lock else ''} 已更新到 {target}"]
    if archived:
        done.append(f"{len(archived)} 份 changelog 已归档")
    if vscode:
        done.append("VS Code 扩展版本已更新")
    print(green(f"✓ {'，'.join(done)}"))
    if not has_lock:
        print(dim("  Cargo.lock 不存在，未同步"))

    count = changed_files()
    print(dim(RULE))
    print("git")
    print(f"工作区 {count} 个文件有更改，将一起提交")
    print(dim(RULE))

    if not confirm(f"提交并打 tag v{target}? [y/n]"):
        print(yellow("已停止，改动保留在工作区，未提交"))
        return

    sha = commit_and_tag(target)
    print(green(f"✓ 已提交 {sha}（release: v{target}），tag v{target} 已创建"))


if __name__ == "__main__":
    try:
        main()
    except KeyboardInterrupt:
        cancelled()
    except ReleaseError as error:
        print(red(f"\n{error}"))
        sys.exit(1)
