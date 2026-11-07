"""E2E —— 执行环境：$cwd / $mode 三级继承、$paths 前置、$mode 三态行为"""


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


def test_mode_default_is_normal(runner):
    """不写 $mode：打印来源行与最终命令，然后直接执行"""
    runner.prepare_alias_json("env", {"hi": "echo hello"})
    r = runner.run("hi")
    assert r.returncode == 0, r.stderr
    assert r.stdout.startswith("\n> ")
    assert "> echo hello" in r.stdout
    assert "hello" in r.stdout


def test_mode_silent_alias_level(runner):
    """$mode=silent：stdout 只剩子进程输出，没有 byk 的 > 提示行"""
    runner.prepare_alias_json("env", {"hi": {"$cmd": "echo hello", "$mode": "silent"}})
    r = runner.run("hi")
    assert r.returncode == 0, r.stderr
    assert r.stdout.strip() == "hello"


def test_mode_silent_file_level_inherited(runner):
    """文件级 $mode=silent 被所有子别名继承"""
    runner.prepare_alias_json("env", {"$mode": "silent", "hi": "echo hello"})
    r = runner.run("hi")
    assert r.returncode == 0, r.stderr
    assert r.stdout.strip() == "hello"


def test_mode_silent_group_level_inherited(runner):
    """分组级 $mode=silent 被组内别名继承"""
    runner.prepare_alias_json("env", {"g": {"$mode": "silent", "hi": "echo hello"}})
    r = runner.run("g.hi")
    assert r.returncode == 0, r.stderr
    assert r.stdout.strip() == "hello"


def test_mode_silent_applies_to_exact_syntax(runner):
    """@@file.key 精确调用同样受 $mode 影响"""
    runner.prepare_alias_json("env", {"$mode": "silent", "hi": "echo hello"})
    r = runner.run("@@env.hi")
    assert r.returncode == 0, r.stderr
    assert r.stdout.strip() == "hello"


def test_mode_normal_overrides_inherited_silent(runner):
    """文件级 silent，别名写 normal 就能恢复提示"""
    runner.prepare_alias_json(
        "env",
        {"$mode": "silent", "loud": {"$cmd": "echo hello", "$mode": "normal"}},
    )
    r = runner.run("loud")
    assert r.returncode == 0, r.stderr
    assert "> echo hello" in r.stdout


def test_mode_silent_overrides_inherited_interactive(runner):
    """文件级 interactive，别名只写 silent 就够——不必再关掉第二个开关"""
    runner.prepare_alias_json(
        "env",
        {"$mode": "interactive", "backup": {"$cmd": "echo quiet", "$mode": "silent"}},
    )
    r = runner.run("backup")
    assert r.returncode == 0, r.stderr
    assert r.stdout.strip() == "quiet"
    assert "Cancelled" not in r.stdout


def test_interactive_shows_preview_then_cancels_without_tty(runner):
    """无 TTY 时交互模式打印预览后取消，不执行命令"""
    runner.prepare_alias_json(
        "env", {"i": {"$cmd": "echo should-not-run", "$mode": "interactive"}}
    )
    r = runner.run("i")
    assert r.returncode == 0, r.stderr
    assert r.stdout.startswith("\n> ")
    assert "${args}" in r.stdout
    assert "Cancelled" in r.stdout


def test_interactive_inherited_from_file_level(runner):
    """文件级 $mode=interactive 被继承"""
    runner.prepare_alias_json("env", {"$mode": "interactive", "i": "echo should-not-run"})
    r = runner.run("i")
    assert r.returncode == 0, r.stderr
    assert "Cancelled" in r.stdout


def test_interactive_inherited_from_group_level(runner):
    """分组级 $mode=interactive 被组内别名继承"""
    runner.prepare_alias_json(
        "env", {"danger": {"$mode": "interactive", "drop": "echo should-not-run"}}
    )
    r = runner.run("danger.drop")
    assert r.returncode == 0, r.stderr
    assert "Cancelled" in r.stdout


def test_interactive_overridden_to_normal(runner):
    """别名级 normal 覆盖文件级 interactive"""
    runner.prepare_alias_json(
        "env",
        {"$mode": "interactive", "direct": {"$cmd": "echo ran-directly", "$mode": "normal"}},
    )
    r = runner.run("direct")
    assert r.returncode == 0, r.stderr
    assert "ran-directly" in r.stdout
    assert "Cancelled" not in r.stdout


def test_mode_invalid_value_warns_and_falls_back_to_normal(runner):
    """非法 $mode 值告警并回落 normal，别名不会整条消失"""
    runner.prepare_alias_json("env", {"hi": {"$cmd": "echo hello", "$mode": "interactiv"}})
    r = runner.run("hi")
    assert r.returncode == 0, r.stderr
    assert "hello" in r.stdout
    assert "$mode must be one of" in r.stderr
    assert "interactiv" in r.stderr
