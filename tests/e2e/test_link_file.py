"""E2E —— 单链接：文件根是数组时，身份借出、内容与相对路径都来自目标文件"""

import json


def bundle(runner, rel: str, data: dict) -> str:
    """在 HOME 下写一个 bundle 文件，返回它相对别名目录的路径"""
    path = runner.home / rel
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(json.dumps(data, ensure_ascii=False), encoding="utf-8")
    return rel


def test_link_resolves_to_target(runner):
    """全局链接指向 bundle，别名进入合并命名空间"""
    bundle(runner, "proj/web/run.byk.json", {"hi": "echo from-bundle"})
    runner.prepare_alias_file("ops", '["../../proj/web/run.byk.json"]')
    r = runner.run("hi")
    assert r.returncode == 0, r.stderr
    assert "from-bundle" in r.stdout


def test_link_relative_cwd_is_target_dir(runner):
    """目标文件的 $cwd 相对路径以目标目录为基准"""
    bundle(runner, "proj/web/run.byk.json", {"$cwd": ".", "where": "pwd"})
    runner.prepare_alias_file("ops", '["../../proj/web/run.byk.json"]')
    r = runner.run("where")
    assert r.returncode == 0, r.stderr
    assert str((runner.home / "proj/web").resolve()) in r.stdout


def test_link_file_dir_is_target_dir(runner):
    """目标文件里的 ${file_dir} 是目标目录，不是别名目录"""
    bundle(runner, "proj/web/run.byk.json", {"dir": "echo ${file_dir}"})
    runner.prepare_alias_file("ops", '["../../proj/web/run.byk.json"]')
    r = runner.run("dir")
    assert r.returncode == 0, r.stderr
    assert str((runner.home / "proj/web").resolve()) in r.stdout
    assert "alias" not in r.stdout


def test_link_exact_call_uses_link_key(runner):
    """身份留在链接文件上：@@ops.key 可以精确调用"""
    bundle(runner, "proj/web/run.byk.json", {"hi": "echo exact-ok"})
    runner.prepare_alias_file("ops", '["../../proj/web/run.byk.json"]')
    r = runner.run("@@ops.hi")
    assert r.returncode == 0, r.stderr
    assert "exact-ok" in r.stdout


def test_link_local_file(runner):
    """当前目录的链接文件同样生效（key 为单 @）"""
    proj = runner.home / "proj"
    proj.mkdir(parents=True, exist_ok=True)
    (proj / "lib.byk.json").write_text('{"hi": "echo local-bundle"}', encoding="utf-8")
    (proj / "p.byk.json").write_text('["lib.byk.json"]', encoding="utf-8")
    r = runner.run("@p.hi", cwd=str(proj))
    assert r.returncode == 0, r.stderr
    assert "local-bundle" in r.stdout


def test_link_relative_path_resolves_against_link_dir(runner):
    """链接路径的相对基准是链接文件所在目录，不是调用目录"""
    bundle(runner, "proj/a/run.byk.json", {"hi": "echo nested-ok"})
    runner.prepare_alias_file("ops", '["../../proj/a/run.byk.json"]')
    elsewhere = runner.home / "elsewhere"
    elsewhere.mkdir(parents=True, exist_ok=True)
    r = runner.run("hi", cwd=str(elsewhere))
    assert r.returncode == 0, r.stderr
    assert "nested-ok" in r.stdout


def test_link_path_accepts_variables(runner):
    """链接路径在链接文件作用域内替换变量"""
    bundle(runner, "proj/web/run.byk.json", {"hi": "echo via-var"})
    runner.prepare_alias_file(
        "ops", '["${file_dir}/../../proj/web/run.byk.json"]'
    )
    r = runner.run("hi")
    assert r.returncode == 0, r.stderr
    assert "via-var" in r.stdout


def test_link_target_var_expands(runner):
    """目标文件的 $var 正常展开（不剥掉）"""
    bundle(
        runner,
        "proj/web/run.byk.json",
        {"$var": {"MSG": "from-target-var"}, "hi": "echo ${MSG}"},
    )
    runner.prepare_alias_file("ops", '["../../proj/web/run.byk.json"]')
    r = runner.run("hi")
    assert r.returncode == 0, r.stderr
    assert "from-target-var" in r.stdout


def test_link_multiple_uses_first_and_warns(runner):
    """多个路径只取第一个，并在 stderr 告知"""
    bundle(runner, "proj/web/a.byk.json", {"hi": "echo first"})
    bundle(runner, "proj/web/b.byk.json", {"hi": "echo second"})
    runner.prepare_alias_file(
        "ops", '["../../proj/web/a.byk.json", "../../proj/web/b.byk.json"]'
    )
    r = runner.run("hi")
    assert r.returncode == 0, r.stderr
    assert "first" in r.stdout
    assert "only the first of 2 link paths" in r.stderr


def test_link_empty_array_is_skipped(runner):
    """空数组告警并跳过，不影响其他别名"""
    runner.prepare_alias_file("ok", '{"hi": "echo still-works"}')
    runner.prepare_alias_file("empty", "[]")
    r = runner.run("hi")
    assert r.returncode == 0, r.stderr
    assert "still-works" in r.stdout
    assert "lists no path" in r.stderr


def test_link_chain_is_rejected(runner):
    """目标文件也是链接 → 报错，但其他文件照常可用"""
    runner.prepare_alias_file("ok", '{"hi": "echo still-works"}')
    bundle(runner, "proj/web/run.byk.json", {"hi": "echo from-bundle"})
    runner.prepare_alias_file("layer1", '["../../proj/web/run.byk.json"]')
    runner.prepare_alias_file("layer2", '["layer1.byk.json"]')
    r = runner.run("hi")
    assert r.returncode == 0, r.stderr
    assert "still-works" in r.stdout
    assert "limited to one level" in r.stderr


def test_link_missing_target_reports_error(runner):
    """目标不存在时报错并跳过该文件"""
    runner.prepare_alias_file("ok", '{"hi": "echo still-works"}')
    runner.prepare_alias_file("gone", '["../../nope/missing.byk.json"]')
    r = runner.run("hi")
    assert r.returncode == 0, r.stderr
    assert "still-works" in r.stdout
    assert "cannot load link target" in r.stderr


def test_link_target_private_honored(runner):
    """目标文件的 $private 不剥：true 会让链接一起退出合并命名空间"""
    bundle(
        runner,
        "proj/web/run.byk.json",
        {"$private": True, "priv": "echo private-ok"},
    )
    runner.prepare_alias_file("ops", '["../../proj/web/run.byk.json"]')
    merged = runner.run("priv")
    assert merged.returncode == 1
    assert "Unrecognized" in merged.stderr
    exact = runner.run("@@ops.priv")
    assert exact.returncode == 0, exact.stderr
    assert "private-ok" in exact.stdout
