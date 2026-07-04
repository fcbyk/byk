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

    def run(self, *args: str) -> subprocess.CompletedProcess[str]:
        """执行 byk 命令，HOME 指向隔离的临时目录"""
        env = {**os.environ, "HOME": str(self.home)}
        return subprocess.run(
            [str(self.binary), *args],
            capture_output=True,
            text=True,
            env=env,
            timeout=10,
        )

    def prepare_alias_file(self, stem: str, content: str) -> Path:
        """在隔离环境中创建别名 JSON 文件"""
        alias_dir = self.byk_dir / "alias"
        alias_dir.mkdir(parents=True, exist_ok=True)
        path = alias_dir / f"{stem}.byk.json"
        path.write_text(content, encoding="utf-8")
        return path


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
