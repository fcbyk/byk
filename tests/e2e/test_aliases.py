"""E2E —— 别名保留变量展开（$python / $venv / $byk）"""


def test_byk_var_expands(runner):
    """$byk 展开为 byk 根目录（隔离 HOME 下的 .byk）"""
    runner.prepare_alias_file("vars", '{"hi": {"$cmd": "echo $byk"}}')
    r = runner.run("hi")
    assert r.returncode == 0
    assert str(runner.byk_dir) in r.stdout


def test_shell_dollar_passthrough(runner):
    """未命中的 $ token 原样透传 shell（$HOME 由 shell 展开）"""
    runner.prepare_alias_file("vars", '{"h": {"$cmd": "echo $HOME"}}')
    r = runner.run("h")
    assert r.returncode == 0
    assert str(runner.home) in r.stdout


def test_python_missing_venv_errors(runner):
    """venv 不存在时 $python 友好报错，退出码非 0，无 shell 乱码"""
    runner.prepare_alias_file("vars", '{"py": {"$cmd": "$python -V"}}')
    r = runner.run("py")
    assert r.returncode != 0
    assert "venv" in r.stderr.lower()
