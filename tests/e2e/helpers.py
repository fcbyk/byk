"""E2E 测试辅助：本地构建插件 wheel、渲染 byk.json 模板。

插件包的构建、打包、清理全部由测试程序控制，
不依赖云端发布，也不在仓库内留下构建产物。
"""

import shutil
import subprocess
import sys
from pathlib import Path


def build_wheel(project_dir: Path, work_dir: Path) -> Path:
    """在 work_dir 内复制 project_dir 并本地构建 wheel，返回 wheel 路径。

    先把项目复制到临时目录再构建，仓库内的夹具目录不留下任何
    构建产物（build/、*.egg-info 等）；wheel 随 work_dir 一并清理。

    构建优先 ``--no-isolation``（复用当前环境的 setuptools，最快）；
    当前环境缺少构建依赖时，回退到隔离构建（从 PyPI 拉取 setuptools）。
    """
    src = work_dir / "src"
    shutil.copytree(
        project_dir,
        src,
        ignore=shutil.ignore_patterns("build", "*.egg-info", "__pycache__"),
    )
    out_dir = work_dir / "dist"
    out_dir.mkdir(parents=True, exist_ok=True)
    attempts = (
        [sys.executable, "-m", "build", "--wheel", "--no-isolation", "--outdir", str(out_dir)],
        [sys.executable, "-m", "build", "--wheel", "--outdir", str(out_dir)],
    )
    last: subprocess.CompletedProcess[str] | None = None
    for cmd in attempts:
        result = subprocess.run(cmd, cwd=str(src), capture_output=True, text=True)
        if result.returncode == 0:
            break
        last = result
    else:
        raise RuntimeError(
            f"wheel build failed in {src}:\n"
            f"STDOUT:\n{last.stdout}\nSTDERR:\n{last.stderr}"
        )

    wheels = sorted(out_dir.glob("*.whl"))
    if len(wheels) != 1:
        raise RuntimeError(f"expected exactly one wheel in {out_dir}, got: {wheels}")
    return wheels[0]


def render_byk_json(template: Path, dest: Path, **replacements: str) -> Path:
    """把模板中的占位符替换为实际值，写入 dest 并返回 dest。"""
    content = template.read_text(encoding="utf-8")
    for placeholder, value in replacements.items():
        content = content.replace(placeholder, value)
    dest.parent.mkdir(parents=True, exist_ok=True)
    dest.write_text(content, encoding="utf-8")
    return dest
