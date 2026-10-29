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
    assert "BYK v" in r.stdout
    assert "Usage:" in r.stdout
    assert "Options:" in r.stdout
    assert "--paths" in r.stdout
    assert "__api__" not in r.stdout


def test_help_lists_aliases_only(runner):
    """内置命令已清空：帮助只列别名，不再出现 add / remove / show"""
    runner.prepare_alias_file("demo", '{"hello": "echo hello"}')
    r = runner.run("--help")
    assert r.returncode == 0
    assert "Aliases:" in r.stdout
    assert "hello" in r.stdout
    for gone in ("add", "remove", "show"):
        assert f"\n  {gone}" not in r.stdout


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
