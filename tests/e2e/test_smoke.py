"""E2E 冒烟测试 —— --help / --version"""


def test_version(runner):
    """--version 打印版本信息并正常退出"""
    r = runner.run("--version")
    assert r.returncode == 0
    assert "byk" in r.stdout
    assert r.stderr == ""


def test_version_short(runner):
    """-v 与 --version 等价"""
    r = runner.run("-v")
    assert r.returncode == 0
    assert "byk" in r.stdout


def test_help(runner):
    """--help 打印用法说明并正常退出"""
    r = runner.run("--help")
    assert r.returncode == 0
    assert r.stdout
    assert "Usage:" in r.stdout
    assert "Options:" in r.stdout
    assert "Commands:" in r.stdout
    assert "add" in r.stdout
    assert "remove" in r.stdout
    assert "show" in r.stdout


def test_help_short(runner):
    """-h 与 --help 等价"""
    r = runner.run("-h")
    assert r.returncode == 0
    assert r.stdout


def test_no_args(runner):
    """无参数时默认显示帮助"""
    r = runner.run()
    assert r.returncode == 0
    assert r.stdout
