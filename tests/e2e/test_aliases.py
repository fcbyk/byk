"""E2E —— 变量替换：内置 ${byk_dir}/${file_dir} 与文件 $var"""


def test_builtin_byk_dir_expands(runner):
    """${byk_dir} 展开为 byk 资源根（隔离 HOME 下的 .byk）"""
    runner.prepare_alias_file("vars", '{"hi": "echo ${byk_dir}"}')
    r = runner.run("hi")
    assert r.returncode == 0
    assert str(runner.byk_dir) in r.stdout


def test_file_dir_points_at_global_alias_dir(runner):
    """${file_dir} 指向别名文件所在目录：全局文件的目录与调用目录无关"""
    work = runner.home / "somewhere"
    work.mkdir(parents=True, exist_ok=True)
    runner.prepare_alias_file("vars", '{"w": "echo ${file_dir}"}')
    r = runner.run("w", cwd=str(work))
    assert r.returncode == 0, r.stderr
    assert str(runner.byk_dir / "alias") in r.stdout
    assert str(work) not in r.stdout


def test_file_dir_independent_of_cwd(runner):
    """$cwd 把命令挪到别处时，${file_dir} 仍是配置文件所在目录"""
    proj = runner.home / "proj"
    build = runner.home / "build"
    for d in (proj, build):
        d.mkdir(parents=True, exist_ok=True)
    (proj / "p.byk.json").write_text(
        '{"w": {"$cmd": "echo ${file_dir}", "$cwd": "%s"}}' % build,
        encoding="utf-8",
    )
    r = runner.run("@p.w", cwd=str(proj))
    assert r.returncode == 0, r.stderr
    assert str(proj) in r.stdout
    assert str(build) not in r.stdout


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
    """\\${byk_dir} 由 shell 输出字面量"""
    runner.prepare_alias_file("vars", r'{"h": "echo \\${byk_dir}"}')
    r = runner.run("h")
    assert r.returncode == 0, r.stderr
    assert "${byk_dir}" in r.stdout


def test_prefix_concatenation(runner):
    """${byk_dir}aaa 是明确的前缀拼接"""
    runner.prepare_alias_file("vars", '{"h": "echo ${byk_dir}aaa"}')
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
    """用户声明优先：$var 里的 byk_dir 覆盖内置值"""
    runner.prepare_alias_file(
        "vars",
        '{"$var": {"byk_dir": "/tmp/custom-byk"}, "h": "echo ${byk_dir}"}',
    )
    r = runner.run("h")
    assert r.returncode == 0
    assert "/tmp/custom-byk" in r.stdout


def test_var_can_extend_builtin(runner):
    """等号右边的 ${byk_dir} 指内置原值：local byk_dir = $byk_dir/tools"""
    runner.prepare_alias_file(
        "vars",
        '{"$var": {"byk_dir": "${byk_dir}/tools"}, "h": "echo ${byk_dir}"}',
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
    """非法变量名被忽略并在 --check 中提示（不参与替换）"""
    runner.prepare_alias_file("vars", '{"$var": {"9bad": "x"}, "h": "echo ok"}')

    r = runner.run("h")
    assert r.returncode == 0
    assert r.stderr == ""
    assert "ok" in r.stdout

    check = runner.run("--check")
    assert check.returncode == 1
    assert "9bad" in check.stdout


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


def test_private_file_needs_scope(runner):
    """$private: true 的文件不参与合并，只能 @file.key 精确调用"""
    work = runner.home / "proj3"
    work.mkdir(parents=True, exist_ok=True)
    (work / "p.byk.json").write_text(
        '{"$private": true, "hidden": "echo hidden"}', encoding="utf-8"
    )

    r = runner.run("hidden", cwd=str(work))
    assert r.returncode != 0

    r2 = runner.run("@p.hidden", cwd=str(work))
    assert r2.returncode == 0, r2.stderr
    assert "hidden" in r2.stdout


def test_retired_builtin_names_are_not_checked(runner):
    """退役变量检查已移除；未定义变量原样交给 shell，${byk_dir} 照常展开"""
    runner.prepare_alias_file("vars", '{"a": "echo ${workdir}/x", "h": "echo ${byk_dir}"}')

    r = runner.run("a")
    assert r.returncode == 0, r.stderr
    assert "/x" in r.stdout

    check = runner.run("--check")
    assert check.returncode == 0
    assert "no longer a built-in" not in check.stdout

    h = runner.run("h")
    assert h.returncode == 0, h.stderr
    assert str(runner.byk_dir) in h.stdout
