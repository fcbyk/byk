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


def test_add_pip_no_target_shows_usage(runner):
    """byk add pip 无 target → 用法帮助，退出码 0"""
    r = runner.run("add", "pip")
    assert r.returncode == 0
    assert "byk add pip" in r.stdout


def test_add_pip_file_not_found(runner):
    """目标文件不存在 → 报错，退出码非 0"""
    r = runner.run("add", "pip", "@nope")
    assert r.returncode != 0
    assert "not found" in r.stderr


def test_add_pip_no_declaration_is_noop(runner):
    """文件无 $pip → 友好 no-op，且不触发 venv 创建（无交互卡住）"""
    runner.prepare_alias_file("nodeps", '{"hi": "echo hello"}')
    r = runner.run("add", "pip", "@@nodeps")
    assert r.returncode == 0
    assert "no $pip" in r.stdout


def test_add_pip_path_form(runner):
    """路径形态：任意位置的 .byk.json 都能作为 target"""
    path = runner.prepare_alias_file("nodeps", '{"hi": "echo hello"}')
    r = runner.run("add", "pip", str(path))
    assert r.returncode == 0
    assert "no $pip" in r.stdout


def test_add_unexpected_target_errors(runner):
    """comp 等功能不接受额外位置参数"""
    r = runner.run("add", "comp", "extra")
    assert r.returncode != 0
    assert "unexpected argument" in r.stderr
