"""E2E —— 调用链保护：自引用 / 互递归 / 逃生口 / 深度兜底

别名的 `$cmd` 可以再次调用 byk，一旦它解析回自己就会无终止地生成进程链
（每层都是独立进程，不存在栈溢出这类自我限制）。这一组测试守住的是：
把「系统级卡死」降级成「一条可读的报错」。
"""


def test_self_reference_is_rejected(runner, byk_binary):
    """别名把自己当命令调用：入口即报错，不再往下生成进程"""
    runner.prepare_alias_json(
        "rec",
        {
            "$mode": "silent",
            "add": {"$cmd": f"{byk_binary} add"},
        },
    )
    r = runner.run("add")
    assert r.returncode == 1
    assert "alias recursion detected" in r.stderr
    assert "@@rec.add -> @@rec.add" in r.stderr


def test_mutual_recursion_is_rejected(runner, byk_binary):
    """A -> B -> A 同样拦住：单值比对看不出来，链路才能"""
    runner.prepare_alias_json(
        "rec",
        {
            "$mode": "silent",
            "a": f"{byk_binary} b",
            "b": f"{byk_binary} a",
        },
    )
    r = runner.run("a")
    assert r.returncode == 1
    assert "alias recursion detected" in r.stderr
    assert "@@rec.a -> @@rec.b -> @@rec.a" in r.stderr


def test_error_is_not_suppressed_by_silent_mode(runner, byk_binary):
    """$mode=silent 只压提示行，压不住这条错误——它正是用来说明卡住的"""
    runner.prepare_alias_json(
        "rec",
        {"add": {"$cmd": f"{byk_binary} add", "$mode": "silent"}},
    )
    r = runner.run("add")
    assert r.returncode == 1
    assert "alias recursion detected" in r.stderr


def test_nested_aliases_still_run(runner, byk_binary):
    """正常编排（不同别名逐层调用）不受影响"""
    runner.prepare_alias_json(
        "rec",
        {
            "$mode": "silent",
            "build": f"{byk_binary} test",
            "test": f"{byk_binary} lint",
            "lint": "echo all-good",
        },
    )
    r = runner.run("build")
    assert r.returncode == 0, r.stderr
    assert r.stdout.strip() == "all-good"


def test_recursive_flag_allows_intentional_nesting(runner, byk_binary):
    """$recursive: true 放行刻意自调用（这里用环境变量让它只绕一圈）"""
    runner.prepare_alias_json(
        "rec",
        {
            "$mode": "silent",
            "r": {
                "$cmd": f'if [ -z "$BY_RAN" ]; then BY_RAN=1 {byk_binary} r; else echo second-pass; fi',
                "$recursive": True,
            },
        },
    )
    r = runner.run("r")
    assert r.returncode == 0, r.stderr
    assert "second-pass" in r.stdout


def test_same_alias_without_flag_is_blocked(runner, byk_binary):
    """同一份配置去掉 $recursive 就回到拦截行为：逃生口是显式选择"""
    runner.prepare_alias_json(
        "rec",
        {
            "$mode": "silent",
            "r": {"$cmd": f'if [ -z "$BY_RAN" ]; then BY_RAN=1 {byk_binary} r; else echo second-pass; fi'},
        },
    )
    r = runner.run("r")
    assert r.returncode == 1
    assert "alias recursion detected" in r.stderr


def test_allow_recursion_env_var(runner, byk_binary, monkeypatch):
    """BYK_ALLOW_RECURSION=1：不改别名配置也能放行"""
    monkeypatch.setenv("BYK_ALLOW_RECURSION", "1")
    runner.prepare_alias_json(
        "rec",
        {
            "$mode": "silent",
            "r": {"$cmd": f'if [ -z "$BY_RAN" ]; then BY_RAN=1 {byk_binary} r; else echo second-pass; fi'},
        },
    )
    r = runner.run("r")
    assert r.returncode == 0, r.stderr
    assert "second-pass" in r.stdout


def test_depth_limit_is_the_backstop(runner, byk_binary):
    """标记被绕过（$recursive）时，深度上限仍然兜底"""
    runner.prepare_alias_json(
        "rec",
        {
            "$mode": "silent",
            "loop": {"$cmd": f"{byk_binary} loop", "$recursive": True},
        },
    )
    r = runner.run("loop")
    assert r.returncode == 1
    assert "depth limit" in r.stderr
