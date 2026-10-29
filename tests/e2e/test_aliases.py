"""E2E —— 别名执行与保留变量展开（$byk / $workdir）"""


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



def test_var_substitution_in_cmd(runner):
    """$var 文件级变量：阶段一替换后命令直接生效"""
    runner.prepare_alias_file(
        "vars",
        '{"$var": {"MSG": "hello-var"}, "say": {"$cmd": "echo $MSG"}}',
    )
    r = runner.run("say")
    assert r.returncode == 0
    assert "hello-var" in r.stdout


def test_install_blocks_ignored(runner):
    """遗留的 $add / $remove 字段不再报错，也不暴露成别名"""
    runner.prepare_alias_file(
        "legacy",
        '{"$add": {"pip": ["rich"]}, "$remove": {"pip": true}, "build": "echo built"}',
    )
    r = runner.run("build")
    assert r.returncode == 0
    assert "built" in r.stdout


def test_local_file_is_callable_without_scope(runner):
    """当前目录的文件默认参与合并，直接 byk <key> 可调用"""
    work = runner.home / "proj"
    work.mkdir(parents=True, exist_ok=True)
    (work / "p.byk.json").write_text('{"hello": "echo local-hi"}', encoding="utf-8")

    r = runner.run("hello", cwd=str(work))
    assert r.returncode == 0, r.stderr
    assert "local-hi" in r.stdout


def test_local_overrides_global(runner):
    """同名时项目文件（优先级 1）覆盖全局（0）"""
    runner.prepare_alias_file("g", '{"hello": "echo global-hi"}')
    work = runner.home / "proj2"
    work.mkdir(parents=True, exist_ok=True)
    (work / "p.byk.json").write_text('{"hello": "echo local-hi"}', encoding="utf-8")

    r = runner.run("hello", cwd=str(work))
    assert r.returncode == 0, r.stderr
    assert "local-hi" in r.stdout

    # 离开该目录后回到全局那份
    r2 = runner.run("hello")
    assert r2.returncode == 0, r2.stderr
    assert "global-hi" in r2.stdout


def test_negative_priority_needs_scope(runner):
    """$priority: -1 的文件不参与合并，只能 @file.key 精确调用"""
    work = runner.home / "proj3"
    work.mkdir(parents=True, exist_ok=True)
    (work / "p.byk.json").write_text(
        '{"$priority": -1, "hidden": "echo hidden"}', encoding="utf-8"
    )

    r = runner.run("hidden", cwd=str(work))
    assert r.returncode != 0

    r2 = runner.run("@p.hidden", cwd=str(work))
    assert r2.returncode == 0, r2.stderr
    assert "hidden" in r2.stdout
