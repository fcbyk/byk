"""E2E —— 帮助渲染（$description / 嵌套分组 / cwd 后缀）与 __api__ 契约"""

import json


def test_help_shows_description(runner):
    """$description 替代命令原文出现在帮助里"""
    runner.prepare_alias_json(
        "r", {"deploy": {"$cmd": "echo d", "$description": "部署到生产环境"}}
    )
    r = runner.run("-h")
    assert r.returncode == 0
    assert "部署到生产环境" in r.stdout
    assert "echo d" not in r.stdout


def test_help_falls_back_to_command(runner):
    """未设置 $description 时显示命令本身"""
    runner.prepare_alias_json("r", {"build": "make build"})
    r = runner.run("-h")
    assert r.returncode == 0
    assert "make build" in r.stdout


def test_help_shows_cwd_suffix(runner):
    """带 $cwd 的别名在帮助里附加目录提示"""
    work = runner.home / "work"
    work.mkdir(parents=True, exist_ok=True)
    runner.prepare_alias_json("r", {"w": {"$cmd": "pwd", "$cwd": str(work)}})
    r = runner.run("-h")
    assert r.returncode == 0
    assert "(~/work)" in r.stdout


def test_help_lists_nested_groups(runner):
    """嵌套分组以点号形式出现在帮助里"""
    runner.prepare_alias_json(
        "r", {"dev": {"build": "echo b", "sub": {"deep": "echo d"}}}
    )
    r = runner.run("-h")
    assert r.returncode == 0
    assert "dev.build" in r.stdout
    assert "dev.sub.deep" in r.stdout


def test_paths_option(runner):
    """--paths 输出 CLI 根目录与别名目录"""
    r = runner.run("--paths")
    assert r.returncode == 0, r.stderr
    assert "CLI Home:" in r.stdout
    assert str(runner.byk_dir) in r.stdout
    assert "Alias:" in r.stdout
    assert str(runner.byk_dir / "alias") in r.stdout


def test_api_lists_requests(runner):
    """无参 __api__ 输出请求清单"""
    r = runner.run("__api__")
    assert r.returncode == 0, r.stderr
    payload = json.loads(r.stdout)
    names = {item["name"] for item in payload["requests"]}
    assert names == {"completion", "complete"}


def test_api_complete_lists_aliases(runner):
    """complete 输出候选，一行一个"""
    runner.prepare_alias_json("r", {"alpha": "echo a", "beta": "echo b"})
    r = runner.run("__api__", "complete", "")
    assert r.returncode == 0, r.stderr
    lines = r.stdout.split("\n")
    assert "alpha" in lines
    assert "beta" in lines


def test_api_complete_at_prefix(runner):
    """@ 前缀只返回精确语法候选"""
    runner.prepare_alias_json("r", {"alpha": "echo a"})
    r = runner.run("__api__", "complete", "@")
    assert r.returncode == 0, r.stderr
    lines = [line for line in r.stdout.split("\n") if line]
    assert lines, "expected at least one @ candidate"
    assert all(line.startswith("@") for line in lines)


def test_api_completion_script_zsh(runner):
    r = runner.run("__api__", "completion", "zsh")
    assert r.returncode == 0, r.stderr
    assert "byk" in r.stdout


def test_api_completion_rejects_unknown_shell(runner):
    r = runner.run("__api__", "completion", "pwsh")
    assert r.returncode == 1
    assert "Unsupported shell" in r.stderr


def test_api_completion_requires_shell_arg(runner):
    r = runner.run("__api__", "completion")
    assert r.returncode == 1
    assert "missing" in r.stderr.lower()


def test_api_rejects_unknown_request(runner):
    r = runner.run("__api__", "nope")
    assert r.returncode == 1
    assert "unknown request" in r.stderr
