"""E2E —— `byk __api__ resolve` 返回别名解析后的完整信息，且不执行命令"""

import json


def test_resolve_simple_alias(runner):
    """合并命名空间：返回最终命令与元信息"""
    runner.prepare_alias_json("r", {"build": "echo built"})
    r = runner.run("__api__", "resolve", "build")
    assert r.returncode == 0, r.stderr
    payload = json.loads(r.stdout)
    assert payload["command"] == "echo built"
    assert payload["mode"] == "normal"
    assert payload["interactive"] is False
    assert payload["key"] == "build"
    assert "source" in payload
    assert "cwd" in payload
    assert "paths" in payload


def test_resolve_does_not_execute(runner):
    """resolve 只解析不执行：命令不产生副作用"""
    marker = runner.home / "marker.txt"
    runner.prepare_alias_json("r", {"run": f"echo ran > {marker}"})
    r = runner.run("__api__", "resolve", "run")
    assert r.returncode == 0, r.stderr
    payload = json.loads(r.stdout)
    assert "ran" in payload["command"]
    assert not marker.exists(), "resolve 不应执行命令"


def test_resolve_placeholder_with_args(runner):
    """占位符按传入 args 解析"""
    runner.prepare_alias_json("r", {"greet": "echo {name}"})
    r = runner.run("__api__", "resolve", "greet", "name", "Alice")
    assert r.returncode == 0, r.stderr
    payload = json.loads(r.stdout)
    assert payload["command"] == "echo Alice"


def test_resolve_interactive_returns_template(runner):
    """交互别名返回未解析模板并标记 interactive"""
    runner.prepare_alias_json(
        "r",
        {"deploy": {"$cmd": "echo {env}", "$mode": "interactive", "$description": "部署"}},
    )
    r = runner.run("__api__", "resolve", "deploy")
    assert r.returncode == 0, r.stderr
    payload = json.loads(r.stdout)
    assert payload["mode"] == "interactive"
    assert payload["interactive"] is True
    assert payload["command"] == "echo {env}"
    assert payload["description"] == "部署"


def test_resolve_reports_mode_normal_by_default(runner):
    """未写 $mode 时报告 normal"""
    runner.prepare_alias_json("r", {"build": "echo built"})
    r = runner.run("__api__", "resolve", "build")
    assert r.returncode == 0, r.stderr
    assert json.loads(r.stdout)["mode"] == "normal"


def test_resolve_reports_mode_inherited(runner):
    """文件级 $mode 继承到别名，并按解析结果返回"""
    runner.prepare_alias_json("r", {"$mode": "silent", "build": "echo built"})
    r = runner.run("__api__", "resolve", "build")
    assert r.returncode == 0, r.stderr
    assert json.loads(r.stdout)["mode"] == "silent"


def test_resolve_reports_mode_and_derived_interactive(runner):
    """mode 是权威字段，interactive 是给旧插件的派生布尔"""
    runner.prepare_alias_json(
        "r", {"deploy": {"$cmd": "echo {env}", "$mode": "interactive"}}
    )
    r = runner.run("__api__", "resolve", "deploy")
    assert r.returncode == 0, r.stderr
    payload = json.loads(r.stdout)
    assert payload["mode"] == "interactive"
    assert payload["interactive"] is True


def test_resolve_exact_global_syntax(runner):
    """@@file.key 精确语法（全局别名目录）"""
    runner.prepare_alias_json("r", {"hello": "echo hi"})
    r = runner.run("__api__", "resolve", "@@r.hello")
    assert r.returncode == 0, r.stderr
    payload = json.loads(r.stdout)
    assert payload["command"] == "echo hi"
    assert payload["source"] == "@@r"


def test_resolve_exact_local_syntax(runner):
    """@file.key 精确语法（当前目录文件）"""
    work = runner.home / "proj"
    work.mkdir(parents=True, exist_ok=True)
    (work / "p.byk.json").write_text('{"hello": "echo local"}', encoding="utf-8")
    r = runner.run("__api__", "resolve", "@p.hello", cwd=str(work))
    assert r.returncode == 0, r.stderr
    payload = json.loads(r.stdout)
    assert payload["command"] == "echo local"
    assert payload["source"] == "@p"


def test_resolve_unknown_alias_fails(runner):
    r = runner.run("__api__", "resolve", "nope")
    assert r.returncode == 1
    assert "not found" in r.stderr


def test_resolve_reports_unloadable_file(runner):
    runner.prepare_alias_file("broken", "{not json")
    r = runner.run("__api__", "resolve", "@@broken.build")
    assert r.returncode == 1
    assert "cannot be loaded" in r.stderr
    assert "invalid JSON" in r.stderr


def test_resolve_rejects_reserved_fields(runner):
    runner.prepare_alias_json(
        "ops",
        {"$custom": "echo custom", "build": {"$cmd": "echo hi"}},
    )
    r = runner.run("__api__", "resolve", "@@ops.build.$cmd")
    assert r.returncode == 1
    assert "reserved $ field" in r.stderr


def test_resolve_requires_key(runner):
    r = runner.run("__api__", "resolve")
    assert r.returncode == 1
    assert "missing" in r.stderr.lower()
