"""E2E 插件生命周期测试 —— 安装 / 执行 / 卸载。

覆盖两种 Python 命令类型：
- ``python``：单文件脚本（downloads 走本地相对路径，全程离线）
- ``python-m``：模块（本地构建 wheel 经 pip 安装，第三方依赖走 PyPI）

python-m 的 wheel 由本测试在 tmp_path 内构建、渲染 byk.json、
安装验证后随临时目录自动清理，不经过任何云端发布。
"""

from pathlib import Path

from .helpers import build_wheel, render_byk_json

FIXTURES = Path(__file__).resolve().parent / "fixtures" / "plugins"


def test_python_plugin(runner):
    """python 类型：安装 → 执行（参数转发）→ 列表可见 → 卸载 → 命令消失"""
    byk_json = FIXTURES / "python" / "byk.json"

    # 安装：downloads 从夹具目录复制，首次创建 venv 需经 stdin 确认
    r = runner.run("add", "py-script", "--file", str(byk_json), input="y\n")
    assert r.returncode == 0, f"install failed:\nSTDOUT:\n{r.stdout}\nSTDERR:\n{r.stderr}"

    # 执行：参数经 byk 转发给脚本
    r = runner.run("py-script", "a", "b")
    assert r.returncode == 0, f"run failed:\nSTDOUT:\n{r.stdout}\nSTDERR:\n{r.stderr}"
    assert "py-script: a b" in r.stdout

    # 已安装插件列表可见
    r = runner.run("show", "plugins")
    assert r.returncode == 0
    assert "py-script" in r.stdout

    # 卸载后命令消失
    r = runner.run("remove", "py-script")
    assert r.returncode == 0, f"remove failed:\nSTDOUT:\n{r.stdout}\nSTDERR:\n{r.stderr}"

    r = runner.run("show", "plugins")
    assert "py-script" not in r.stdout


def test_python_m_plugin(runner, tmp_path):
    """python-m 类型：本地构建 wheel → 安装 → 执行 → 卸载"""
    project = FIXTURES / "python-m"

    # 构建：本地打 wheel（在临时目录内构建，夹具目录零残留）
    wheel = build_wheel(project, tmp_path)

    # 打包：模板注入本地 wheel 路径（模板中已含 file:// 前缀）
    byk_json = render_byk_json(
        project / "byk.json",
        tmp_path / "byk.json",
        __WHEEL__=str(wheel),
    )

    # 安装：pip 装本地 wheel（click 等依赖从 PyPI 拉取），首次创建 venv 需确认
    r = runner.run("add", "py-module", "--file", str(byk_json), input="y\n")
    assert r.returncode == 0, f"install failed:\nSTDOUT:\n{r.stdout}\nSTDERR:\n{r.stderr}"

    # 执行：python -m py_module，参数经 click 解析
    r = runner.run("py-module", "--name", "ci")
    assert r.returncode == 0, f"run failed:\nSTDOUT:\n{r.stdout}\nSTDERR:\n{r.stderr}"
    assert "hello ci" in r.stdout

    # 卸载
    r = runner.run("remove", "py-module")
    assert r.returncode == 0, f"remove failed:\nSTDOUT:\n{r.stdout}\nSTDERR:\n{r.stderr}"
