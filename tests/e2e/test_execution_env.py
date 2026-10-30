"""E2E —— 执行环境：$cwd 三级继承、$paths 前置、$interactive 预览与继承"""


def make_dir(runner, name: str):
    path = runner.home / name
    path.mkdir(parents=True, exist_ok=True)
    return path


def test_cwd_alias_level(runner):
    """别名级 $cwd 决定命令的落地目录"""
    target = make_dir(runner, "work")
    runner.prepare_alias_json("env", {"w": {"$cmd": "pwd", "$cwd": str(target)}})
    r = runner.run("w")
    assert r.returncode == 0, r.stderr
    assert str(target.resolve()) in r.stdout


def test_cwd_group_level_inherited(runner):
    """分组级 $cwd 被子别名继承"""
    outer = make_dir(runner, "outer")
    inner = make_dir(runner, "inner")
    runner.prepare_alias_json(
        "env",
        {"$cwd": str(outer), "g": {"$cwd": str(inner), "w": "pwd"}},
    )
    r = runner.run("g.w")
    assert r.returncode == 0, r.stderr
    assert str(inner.resolve()) in r.stdout


def test_cwd_file_level_inherited(runner):
    """文件级 $cwd 被所有子别名继承"""
    target = make_dir(runner, "root")
    runner.prepare_alias_json("env", {"$cwd": str(target), "w": "pwd"})
    r = runner.run("w")
    assert r.returncode == 0, r.stderr
    assert str(target.resolve()) in r.stdout


def test_cwd_alias_overrides_group(runner):
    """越靠近别名优先级越高：别名级覆盖分组级"""
    group_dir = make_dir(runner, "group")
    own_dir = make_dir(runner, "own")
    runner.prepare_alias_json(
        "env",
        {
            "g": {
                "$cwd": str(group_dir),
                "w": {"$cmd": "pwd", "$cwd": str(own_dir)},
            }
        },
    )
    r = runner.run("g.w")
    assert r.returncode == 0, r.stderr
    assert str(own_dir.resolve()) in r.stdout
    assert str(group_dir.resolve()) not in r.stdout


def test_paths_prepends_custom_dir(runner):
    """$paths 把目录前置到 PATH，别名可直接调用其中的可执行文件"""
    bin_dir = make_dir(runner, "bin")
    script = bin_dir / "greet.sh"
    script.write_text("#!/bin/sh\necho from-custom-path\n", encoding="utf-8")
    script.chmod(0o755)

    runner.prepare_alias_json(
        "env", {"$paths": [str(bin_dir)], "greet": "greet.sh"}
    )
    r = runner.run("greet")
    assert r.returncode == 0, r.stderr
    assert "from-custom-path" in r.stdout


def test_paths_relative_to_config_file(runner):
    """相对 $paths 以配置文件所在目录为基准，而非调用时的当前目录"""
    alias_dir = runner.byk_dir / "alias"
    alias_dir.mkdir(parents=True, exist_ok=True)
    bin_dir = alias_dir / "bin2"
    bin_dir.mkdir(exist_ok=True)
    script = bin_dir / "hi.sh"
    script.write_text("#!/bin/sh\necho hi-from-bin2\n", encoding="utf-8")
    script.chmod(0o755)

    runner.prepare_alias_json("env", {"$paths": ["./bin2"], "hi": "hi.sh"})

    other_cwd = make_dir(runner, "elsewhere")
    r = runner.run("hi", cwd=str(other_cwd))
    assert r.returncode == 0, r.stderr
    assert "hi-from-bin2" in r.stdout


def test_interactive_shows_preview_then_cancels_without_tty(runner):
    """无 TTY 时交互模式打印预览后取消，不执行命令"""
    runner.prepare_alias_json(
        "env", {"i": {"$cmd": "echo should-not-run", "$interactive": True}}
    )
    r = runner.run("i")
    assert r.returncode == 0, r.stderr
    assert "${args}" in r.stdout
    assert "Cancelled" in r.stdout


def test_interactive_inherited_from_file_level(runner):
    """文件级 $interactive 被继承"""
    runner.prepare_alias_json(
        "env", {"$interactive": True, "i": "echo should-not-run"}
    )
    r = runner.run("i")
    assert r.returncode == 0, r.stderr
    assert "Cancelled" in r.stdout


def test_interactive_overridden_to_false(runner):
    """别名级显式 false 覆盖文件级 true"""
    runner.prepare_alias_json(
        "env",
        {
            "$interactive": True,
            "direct": {"$cmd": "echo ran-directly", "$interactive": False},
        },
    )
    r = runner.run("direct")
    assert r.returncode == 0, r.stderr
    assert "ran-directly" in r.stdout
    assert "Cancelled" not in r.stdout
