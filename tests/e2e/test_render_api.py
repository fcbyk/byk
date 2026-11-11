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
    assert names == {"completion", "complete", "resolve", "paths"}


def test_api_paths_json(runner):
    """无参 paths 输出两条目录的 JSON"""
    r = runner.run("__api__", "paths")
    assert r.returncode == 0, r.stderr
    payload = json.loads(r.stdout)
    assert payload == {
        "root": str(runner.byk_dir),
        "alias": str(runner.byk_dir / "alias"),
    }


def test_api_paths_single_value_is_bare(runner):
    """paths <name> 只输出裸字符串：无引号、无转义、结尾一个换行，可直接进 $()"""
    r = runner.run("__api__", "paths", "root")
    assert r.returncode == 0, r.stderr
    assert r.stdout == f"{runner.byk_dir}\n"

    r = runner.run("__api__", "paths", "alias")
    assert r.returncode == 0, r.stderr
    assert r.stdout == f"{runner.byk_dir / 'alias'}\n"


def test_api_paths_does_not_create_dirs(runner):
    """paths 只回答位置，不顺手创建目录"""
    r = runner.run("__api__", "paths")
    assert r.returncode == 0, r.stderr
    assert not runner.byk_dir.exists()


def test_api_paths_rejects_unknown_name(runner):
    r = runner.run("__api__", "paths", "bin")
    assert r.returncode == 1
    assert 'unknown name "bin"' in r.stderr
    assert "root, alias" in r.stderr
    assert r.stdout == ""


def test_api_paths_rejects_extra_args(runner):
    r = runner.run("__api__", "paths", "root", "alias")
    assert r.returncode == 1
    assert "too many arguments" in r.stderr


def test_api_complete_lists_aliases(runner):
    """complete 输出候选，一行一个"""
    runner.prepare_alias_json("r", {"alpha": "echo a", "beta": "echo b"})
    r = runner.run("__api__", "complete", "")
    assert r.returncode == 0, r.stderr
    lines = r.stdout.split("\n")
    assert "alpha" in lines
    assert "beta" in lines


def test_api_complete_default_has_no_descriptions(runner):
    r = runner.run("__api__", "complete", "-")
    assert r.returncode == 0, r.stderr
    lines = r.stdout.splitlines()
    assert "--help" in lines
    assert not any("\t" in line for line in lines)


def test_api_complete_with_descriptions_flags(runner):
    r = runner.run("__api__", "complete", "--with-descriptions", "-")
    assert r.returncode == 0, r.stderr
    lines = r.stdout.splitlines()
    assert "--help\tPrint help" in lines
    assert "--version\tPrint version" in lines
    assert "--paths\tPrint CLI paths and file locations" in lines


def test_api_complete_with_descriptions_file_command_verbs(runner):
    r = runner.run("__api__", "complete", "--with-descriptions", "@@ops..")
    assert r.returncode == 0, r.stderr
    assert r.stdout.splitlines() == [
        "@@ops..init\tCreate an alias file",
        "@@ops..destroy\tRemove the alias file",
        "@@ops..clear\tReset this file to an empty object",
        "@@ops..json\tShow the alias file JSON",
        "@@ops..path\tPrint the absolute path",
        "@@ops..rename\tRename the alias file",
        "@@ops..link\tPoint this file at another alias file",
        "@@ops..help\tShow this help",
    ]


def test_api_complete_with_descriptions_keeps_dynamic_candidates_plain(runner):
    runner.prepare_alias_json("r", {"alpha": "echo a"})
    r = runner.run("__api__", "complete", "--with-descriptions", "")
    assert r.returncode == 0, r.stderr
    assert "alpha" in r.stdout.splitlines()


def test_api_completion_script_zsh_uses_descriptions(runner):
    r = runner.run("__api__", "completion", "zsh")
    assert r.returncode == 0, r.stderr
    assert "--with-descriptions" in r.stdout
    assert "_describe 'byk' items" in r.stdout


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
