"""E2E 插件生命周期测试 —— 安装 / 执行。

byk.json 插件系统已废弃，插件迁移到 *.byk.json（$cmd + $pip + $assets）。
覆盖两种 Python 命令类型的等价迁移：
- ``python``：单文件脚本（$assets 下载 + 执行，纯标准库，走系统 python3）
- ``python-m``：模块（$pip 装本地 wheel + $python -m 执行，走 byk 托管 venv）

python-m 的 wheel 由本测试在 tmp_path 内构建、渲染 *.byk.json、
安装验证后随临时目录自动清理，不经过任何云端发布。
"""

from pathlib import Path

from .helpers import build_wheel, render_byk_json

FIXTURES = Path(__file__).resolve().parent / "fixtures" / "plugins"


def test_python_plugin(runner):
    """python 类型：$assets 下载脚本 + 执行（参数经 byk 转发）"""
    byk_json = FIXTURES / "python" / "py-script.byk.json"

    # 安装：$assets 从夹具目录复制到 $byk/plugins/py-script/，副本写入全局
    r = runner.run("add", "-f", str(byk_json))
    assert r.returncode == 0, f"install failed:\nSTDOUT:\n{r.stdout}\nSTDERR:\n{r.stderr}"

    # 执行：参数经 byk 转发给脚本
    r = runner.run("py-script", "a", "b")
    assert r.returncode == 0, f"run failed:\nSTDOUT:\n{r.stdout}\nSTDERR:\n{r.stderr}"
    assert "py-script: a b" in r.stdout


def test_python_m_plugin(runner, tmp_path):
    """python-m 类型：本地构建 wheel → $pip 安装 → $python -m 执行"""
    project = FIXTURES / "python-m"

    # 本地打 wheel（临时目录内构建，夹具目录零残留）
    wheel = build_wheel(project, tmp_path)

    # 模板注入本地 wheel 路径（模板中已含 file:// 前缀）
    byk_json = render_byk_json(
        project / "py-module.byk.json",
        tmp_path / "py-module.byk.json",
        __WHEEL__=str(wheel),
    )

    # 安装：$pip 装本地 wheel（click 依赖走 PyPI），首次创建 venv 需确认
    r = runner.run("add", "-f", str(byk_json), input="y\n")
    assert r.returncode == 0, f"install failed:\nSTDOUT:\n{r.stdout}\nSTDERR:\n{r.stderr}"

    # 执行：$python -m py_module，参数经 click 解析
    r = runner.run("py-module", "--name", "ci")
    assert r.returncode == 0, f"run failed:\nSTDOUT:\n{r.stdout}\nSTDERR:\n{r.stderr}"
    assert "hello ci" in r.stdout
