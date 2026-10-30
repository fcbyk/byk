import json
import os
import subprocess
from pathlib import Path
from tempfile import TemporaryDirectory

import pytest

PROJECT_ROOT = Path(__file__).resolve().parents[2]


class BykRunner:
    """封装 byk 二进制执行，统一管理 HOME 隔离和输出捕获"""

    def __init__(self, binary: Path, home: str):
        self.binary = binary
        self.home = Path(home)
        self.byk_dir = self.home / ".byk"

    def run(
        self,
        *args: str,
        input: str | None = None,
        cwd: str | None = None,
    ) -> subprocess.CompletedProcess[str]:
        """执行 byk 命令，HOME 指向隔离的临时目录

        input: 经 stdin 喂入的内容（如首次创建 venv 的确认）。
                为 None 时 stdin 指向 DEVNULL，避免子进程继承终端
                导致交互提示卡住测试。
        cwd:    子进程工作目录（默认继承 pytest 的 cwd）。
                测试全局文件“缺省根 = 工作目录”时须传隔离目录，
                避免污染项目根。
        """
        env = {**os.environ, "HOME": str(self.home)}
        return subprocess.run(
            [str(self.binary), *args],
            capture_output=True,
            text=True,
            input=input,
            stdin=None if input is not None else subprocess.DEVNULL,
            env=env,
            cwd=cwd,
            timeout=120,
        )

    def prepare_alias_file(self, stem: str, content: str) -> Path:
        """在隔离环境中创建别名 JSON 文件"""
        alias_dir = self.byk_dir / "alias"
        alias_dir.mkdir(parents=True, exist_ok=True)
        path = alias_dir / f"{stem}.byk.json"
        path.write_text(content, encoding="utf-8")
        return path

    def prepare_alias_json(self, stem: str, data: dict) -> Path:
        """以 Python dict 写别名文件，免去手写 JSON 转义"""
        return self.prepare_alias_file(stem, json.dumps(data, ensure_ascii=False))


@pytest.fixture(scope="session")
def byk_binary() -> Path:
    """编译 byk 二进制，整个 session 只编译一次"""
    result = subprocess.run(
        ["cargo", "build"],
        cwd=str(PROJECT_ROOT),
        capture_output=True,
        text=True,
    )
    if result.returncode != 0:
        pytest.fail(
            f"cargo build 失败:\nSTDOUT:\n{result.stdout}\nSTDERR:\n{result.stderr}"
        )

    binary = PROJECT_ROOT / "target" / "debug" / "byk"
    if not binary.exists():
        pytest.fail(f"编译产物不存在: {binary}")

    return binary


@pytest.fixture
def runner(byk_binary: Path):
    """隔离环境的 byk runner，每个测试独立"""
    with TemporaryDirectory(prefix="byk_e2e_") as tmp:
        yield BykRunner(byk_binary, tmp)
