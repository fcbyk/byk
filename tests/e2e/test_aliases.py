"""E2E —— 变量替换：内置 ${byk}/${workdir} 与文件 $var"""


def test_builtin_byk_expands(runner):
    """${byk} 展开为 byk 根目录（隔离 HOME 下的 .byk）"""
    runner.prepare_alias_file("vars", '{"hi": "echo ${byk}"}')
    r = runner.run("hi")
    assert r.returncode == 0
    assert str(runner.byk_dir) in r.stdout


def test_builtin_workdir_expands(runner):
    """${workdir} 展开为调用 byk 时的目录"""
    work = runner.home / "wd"
    work.mkdir(parents=True, exist_ok=True)
    runner.prepare_alias_file("vars", '{"w": "echo ${workdir}"}')
    r = runner.run("w", cwd=str(work))
    assert r.returncode == 0, r.stderr
    assert str(work) in r.stdout


def test_bare_dollar_left_to_shell(runner):
    """裸 $xxx 交给 shell，byk 不识别"""
    runner.prepare_alias_file("vars", '{"h": "echo X$byk-Y"}')
    r = runner.run("h")
    assert r.returncode == 0
    assert "X-Y" in r.stdout
    assert str(runner.byk_dir) not in r.stdout


def test_shell_var_passthrough(runner):
    """$HOME 仍由 shell 展开"""
    runner.prepare_alias_file("vars", '{"h": "echo $HOME"}')
    r = runner.run("h")
    assert r.returncode == 0
    assert str(runner.home) in r.stdout


def test_escaped_brace_left_to_shell(runner):
    """\\${byk} 由 shell 输出字面量"""
    runner.prepare_alias_file("vars", r'{"h": "echo \\${byk}"}')
    r = runner.run("h")
    assert r.returncode == 0, r.stderr
    assert "${byk}" in r.stdout


def test_prefix_concatenation(runner):
    """${byk}aaa 是明确的前缀拼接"""
    runner.prepare_alias_file("vars", '{"h": "echo ${byk}aaa"}')
    r = runner.run("h")
    assert r.returncode == 0
    assert f"{runner.byk_dir}aaa" in r.stdout


def test_var_substitution_in_cmd(runner):
    """$var 文件级变量在扫描期替换完成"""
    runner.prepare_alias_file(
        "vars",
        '{"$var": {"MSG": "hello-var"}, "say": {"$cmd": "echo ${MSG}"}}',
    )
    r = runner.run("say")
    assert r.returncode == 0
    assert "hello-var" in r.stdout


def test_var_overrides_builtin(runner):
    """用户声明优先：$var 里的 byk 覆盖内置值"""
    runner.prepare_alias_file(
        "vars",
        '{"$var": {"byk": "/tmp/custom-byk"}, "h": "echo ${byk}"}',
    )
    r = runner.run("h")
    assert r.returncode == 0
    assert "/tmp/custom-byk" in r.stdout


def test_var_can_extend_builtin(runner):
    """等号右边的 ${byk} 指内置原值：local byk = $byk/tools"""
    runner.prepare_alias_file(
        "vars",
        '{"$var": {"byk": "${byk}/tools"}, "h": "echo ${byk}"}',
    )
    r = runner.run("h")
    assert r.returncode == 0
    assert f"{runner.byk_dir}/tools" in r.stdout


def test_args_placeholder_not_shadowed(runner):
    """${args} 属于参数占位符，不被变量层吃掉"""
    runner.prepare_alias_file("vars", '{"say": "echo ${args}"}')
    r = runner.run("say", "a", "b")
    assert r.returncode == 0
    assert "a b" in r.stdout


def test_invalid_var_name_warns(runner):
    """非法变量名被忽略并提示（不参与替换）"""
    runner.prepare_alias_file("vars", '{"$var": {"9bad": "x"}, "h": "echo ok"}')
    r = runner.run("h")
    assert r.returncode == 0
    assert "9bad" in r.stderr
    assert "ok" in r.stdout


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
