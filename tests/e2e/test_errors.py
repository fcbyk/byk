"""E2E —— 错误路径与退出码：非法文件/键被跳过、精确语法报错、退出码透传"""


def test_invalid_filename_skipped(runner):
    """文件名含点号不合法，整份文件被静默跳过"""
    runner.prepare_alias_file("aaa.bbb", '{"ghost": "echo ghost"}')
    r = runner.run("ghost")
    assert r.returncode == 1
    assert "ghost" not in runner.run("-h").stdout


def test_key_with_dot_ignored(runner):
    """key 不能包含点号，违规则该条目被忽略"""
    runner.prepare_alias_json("e", {"a.b": "echo nope", "ok": "echo ok"})
    assert runner.run("a.b").returncode == 1

    good = runner.run("ok")
    assert good.returncode == 0, good.stderr


def test_key_with_at_sign_ignored(runner):
    """key 不能包含 @，违规则该条目被忽略"""
    runner.prepare_alias_json("e", {"x@y": "echo nope", "ok": "echo ok"})
    assert runner.run("x@y").returncode == 1

    good = runner.run("ok")
    assert good.returncode == 0, good.stderr


def test_broken_json_skipped(runner):
    """JSON 损坏的文件被跳过，不影响其他文件"""
    runner.prepare_alias_file("broken", "{ this is not json")
    runner.prepare_alias_json("fine", {"ok": "echo still-works"})

    r = runner.run("ok")
    assert r.returncode == 0, r.stderr
    assert "still-works" in r.stdout


def test_unknown_alias_reports_and_exits_1(runner):
    r = runner.run("no-such-alias")
    assert r.returncode == 1
    assert "no-such-alias" in r.stderr


def test_exact_syntax_missing_file(runner):
    r = runner.run("@nofile.build")
    assert r.returncode == 1
    assert "Alias file not found" in r.stderr


def test_exact_syntax_missing_key(runner):
    runner.prepare_alias_json("known", {"other": "echo x"})
    r = runner.run("@@known.build")
    assert r.returncode == 1
    assert "Alias key not found" in r.stderr


def test_exit_code_propagated(runner):
    """命令自身的退出码原样返回"""
    runner.prepare_alias_json("e", {"fail": "exit 3"})
    r = runner.run("fail")
    assert r.returncode == 3


def test_command_not_found_exit_127(runner):
    runner.prepare_alias_json("e", {"ghost": "definitely-not-a-real-command-9f3a"})
    r = runner.run("ghost")
    assert r.returncode == 127
