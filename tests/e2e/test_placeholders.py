"""E2E —— 参数占位符：具名 / 可选透传 / 条件 / 位置 / 剩余参数 / 裸 {}"""


def test_named_placeholder_takes_flag_and_value(runner):
    """{--src} 消耗 `--src` 与其后的值"""
    runner.prepare_alias_json("ph", {"copy": "echo [{--src}][{--dst}]"})
    r = runner.run("copy", "--src", "a", "--dst", "b")
    assert r.returncode == 0, r.stderr
    assert "[a][b]" in r.stdout


def test_named_placeholder_absent_disappears(runner):
    """取不到值时静默消失，不报错"""
    runner.prepare_alias_json("ph", {"copy": "echo [{--src}][{--dst}]"})
    r = runner.run("copy")
    assert r.returncode == 0, r.stderr
    assert "[][]" in r.stdout


def test_optional_placeholder_renders_flag_and_value(runner):
    """{{--mode}} 有值时渲染成 `--mode value`"""
    runner.prepare_alias_json("ph", {"b": "echo A{{--mode}}B"})
    r = runner.run("b", "--mode", "prod")
    assert r.returncode == 0, r.stderr
    assert "A--mode prodB" in r.stdout


def test_optional_placeholder_absent_vanishes(runner):
    runner.prepare_alias_json("ph", {"b": "echo A{{--mode}}B"})
    r = runner.run("b")
    assert r.returncode == 0, r.stderr
    assert "AB" in r.stdout


def test_conditional_ternary_true_branch(runner):
    runner.prepare_alias_json(
        "ph", {"t": "echo [{debug?--verbose:--quiet}]"}
    )
    r = runner.run("t", "debug")
    assert r.returncode == 0, r.stderr
    assert "[--verbose]" in r.stdout


def test_conditional_ternary_false_branch(runner):
    runner.prepare_alias_json(
        "ph", {"t": "echo [{debug?--verbose:--quiet}]"}
    )
    r = runner.run("t")
    assert r.returncode == 0, r.stderr
    assert "[--quiet]" in r.stdout


def test_conditional_simple_form(runner):
    """{--open?--open} 最简形式：命中渲染 key 本身"""
    runner.prepare_alias_json("ph", {"o": "echo [{--open?--open}]"})
    hit = runner.run("o", "--open")
    assert hit.returncode == 0, hit.stderr
    assert "[--open]" in hit.stdout

    miss = runner.run("o")
    assert miss.returncode == 0, miss.stderr
    assert "[]" in miss.stdout


def test_positional_placeholders(runner):
    """${N} 取原始参数的第 N 个（0 基）"""
    runner.prepare_alias_json("ph", {"p": "echo [${0}][${2}]"})
    r = runner.run("p", "a", "b", "c")
    assert r.returncode == 0, r.stderr
    assert "[a][c]" in r.stdout


def test_rest_args_excludes_consumed(runner):
    """${...args} 去掉已被其他占位符消耗的位置"""
    runner.prepare_alias_json("ph", {"r": "echo [{--src}][${...args}]"})
    r = runner.run("r", "--src", "foo", "x", "y")
    assert r.returncode == 0, r.stderr
    assert "[foo][x y]" in r.stdout


def test_args_placeholder_takes_everything(runner):
    runner.prepare_alias_json("ph", {"a": "echo [${args}]"})
    r = runner.run("a", "x", "y")
    assert r.returncode == 0, r.stderr
    assert "[x y]" in r.stdout


def test_no_placeholder_appends_all_args(runner):
    """无占位符时等价于末尾隐式追加 ${args}"""
    runner.prepare_alias_json("ph", {"n": "echo [plain]"})
    r = runner.run("n", "q", "r")
    assert r.returncode == 0, r.stderr
    assert "[plain] q r" in r.stdout


def test_bare_braces_untouched(runner):
    """{} 是 find 的语法，不被当作占位符"""
    runner.prepare_alias_json("ph", {"f": "echo {}"})
    r = runner.run("f")
    assert r.returncode == 0, r.stderr
    assert "{}" in r.stdout


def test_placeholder_repeated_reuses_value(runner):
    """同一占位符多次出现只消耗一次，值复用"""
    runner.prepare_alias_json("ph", {"rep": "echo [${0}][${0}][${0}]"})
    r = runner.run("rep", "hi")
    assert r.returncode == 0, r.stderr
    assert "[hi][hi][hi]" in r.stdout
