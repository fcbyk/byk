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


SEPARATOR = "-" * 27


def assert_blank_padded(stdout):
    """--paths 输出前后各一空行，内部不留空行"""
    lines = stdout.splitlines()
    assert lines[0] == ""
    assert lines[-1] == ""
    assert all(line for line in lines[1:-1])
    return lines


def test_paths_option(runner):
    """--paths 输出 CLI 根目录、别名目录与工作目录"""
    r = runner.run("--paths")
    assert r.returncode == 0, r.stderr
    lines = assert_blank_padded(r.stdout)
    assert any(line.startswith("CLI Home: ") for line in lines)
    assert str(runner.byk_dir) in r.stdout
    assert any(line.startswith("Alias: ") for line in lines)
    assert str(runner.byk_dir / "alias") in r.stdout
    assert any(line.startswith("Workdir: ") for line in lines)


def test_paths_lists_global_and_local_alias_files(runner, tmp_path):
    """--paths 列出两处目录下的别名文件，只校验文件名不解析内容"""
    global_file = runner.prepare_alias_json("g", {"build": "echo g"})
    local_file = tmp_path / "l.byk.json"
    local_file.write_text(json.dumps({"run": "echo l"}), encoding="utf-8")
    (tmp_path / "broken.byk.json").write_text("not json", encoding="utf-8")
    (tmp_path / "bad.name.byk.json").write_text("{}", encoding="utf-8")

    r = runner.run("--paths", cwd=str(tmp_path))
    assert r.returncode == 0, r.stderr
    lines = assert_blank_padded(r.stdout)

    assert f"Workdir: {tmp_path}" in lines
    assert lines.count(SEPARATOR) == 2
    global_idx = lines.index(f"@@g: {global_file}")
    local_idx = lines.index(f"@l: {local_file}")
    assert lines.index(SEPARATOR) < global_idx
    assert lines.index(SEPARATOR, global_idx) < local_idx
    # 内容损坏的文件仍然列出；文件名非法的文件不列出
    assert any(
        line.startswith("@broken: ") and "broken.byk.json" in line for line in lines
    )
    assert "bad.name" not in r.stdout


def test_paths_lists_broken_symlink(runner, tmp_path):
    """断链仍是可寻址的目录项，位置清单列出它；内容问题交给 --check"""
    link = tmp_path / "ops.byk.json"
    link.symlink_to(tmp_path / "missing.byk.json")

    r = runner.run("--paths", cwd=str(tmp_path))
    assert r.returncode == 0, r.stderr
    assert f"@ops: {link}" in r.stdout


def test_paths_omits_empty_alias_file_groups(runner, tmp_path):
    """没有别名文件时整组都不打印"""
    r = runner.run("--paths", cwd=str(tmp_path))
    assert r.returncode == 0, r.stderr
    lines = assert_blank_padded(r.stdout)
    assert SEPARATOR not in lines
    assert len(lines) == 5


def test_paths_omits_only_missing_group(runner, tmp_path):
    """只有一边有文件时，只打印那一边的分隔线"""
    (tmp_path / "l.byk.json").write_text("{}", encoding="utf-8")
    r = runner.run("--paths", cwd=str(tmp_path))
    lines = assert_blank_padded(r.stdout)
    assert lines.count(SEPARATOR) == 1
    assert any(line.startswith("@l: ") for line in lines)
    assert not any(line.startswith("@@") for line in lines)

    runner.prepare_alias_json("g", {"build": "echo g"})
    empty = tmp_path / "empty"
    empty.mkdir()
    r = runner.run("--paths", cwd=str(empty))
    lines = assert_blank_padded(r.stdout)
    assert lines.count(SEPARATOR) == 1
    assert any(line.startswith("@@g: ") for line in lines)
    assert not any(line.startswith("@") and not line.startswith("@@") for line in lines)


def test_api_lists_requests(runner):
    """无参 __api__ 输出版本与请求清单"""
    r = runner.run("__api__")
    assert r.returncode == 0, r.stderr
    payload = json.loads(r.stdout)
    version = runner.run("-v").stdout.split()[1]
    assert payload["version"] == version
    names = {item["name"] for item in payload["requests"]}
    assert names == {
        "completion",
        "complete",
        "resolve",
        "paths",
        "file",
        "expand",
        "check",
    }


def test_api_paths_json(runner, tmp_path):
    """无参 paths 输出目录与别名文件清单的 JSON"""
    r = runner.run("__api__", "paths", cwd=str(tmp_path))
    assert r.returncode == 0, r.stderr
    payload = json.loads(r.stdout)
    assert payload == {
        "root": str(runner.byk_dir),
        "alias": str(runner.byk_dir / "alias"),
        "workdir": str(tmp_path.resolve()),
        "aliasFiles": [],
    }


def test_api_paths_json_lists_alias_files(runner, tmp_path):
    """aliasFiles 是路径数组，先全局后本地，按 key 排序；只校验文件名不解析内容"""
    global_file = runner.prepare_alias_json("g", {"build": "echo g"})
    local_file = tmp_path / "l.byk.json"
    local_file.write_text(json.dumps({"run": "echo l"}), encoding="utf-8")
    (tmp_path / "broken.byk.json").write_text("not json", encoding="utf-8")
    (tmp_path / "bad.name.byk.json").write_text("{}", encoding="utf-8")

    r = runner.run("__api__", "paths", cwd=str(tmp_path))
    assert r.returncode == 0, r.stderr
    workdir = tmp_path.resolve()
    assert json.loads(r.stdout)["aliasFiles"] == [
        str(global_file),
        str(workdir / "broken.byk.json"),
        str(workdir / "l.byk.json"),
    ]


def test_api_file_local(runner, tmp_path):
    """file 解析本地文件 key，不要求文件存在"""
    r = runner.run("__api__", "file", "@p", cwd=str(tmp_path))
    assert r.returncode == 0, r.stderr
    workdir = tmp_path.resolve()
    assert json.loads(r.stdout) == {
        "key": "@p",
        "scope": "local",
        "path": str(workdir / "p.byk.json"),
        "dir": str(workdir),
    }
    assert not (workdir / "p.byk.json").exists()


def test_api_file_global(runner, tmp_path):
    """file 解析全局文件 key"""
    r = runner.run("__api__", "file", "@@ops", cwd=str(tmp_path))
    assert r.returncode == 0, r.stderr
    alias_dir = runner.byk_dir / "alias"
    assert json.loads(r.stdout) == {
        "key": "@@ops",
        "scope": "global",
        "path": str(alias_dir / "ops.byk.json"),
        "dir": str(alias_dir),
    }
    assert not runner.byk_dir.exists()


def test_api_file_anonymous(runner, tmp_path):
    """file 支持匿名文件 key"""
    r = runner.run("__api__", "file", "@", cwd=str(tmp_path))
    assert r.returncode == 0, r.stderr
    assert json.loads(r.stdout)["path"] == str(tmp_path.resolve() / ".byk.json")

    r = runner.run("__api__", "file", "@@", cwd=str(tmp_path))
    assert r.returncode == 0, r.stderr
    assert json.loads(r.stdout)["path"] == str(
        runner.byk_dir / "alias" / ".byk.json"
    )


def test_api_file_list(runner, tmp_path):
    """--list 返回对象数组，与 paths.aliasFiles 同序；坏文件列出，非法文件名不列"""
    global_file = runner.prepare_alias_json("g", {"build": "echo g"})
    local_file = tmp_path / "l.byk.json"
    local_file.write_text(json.dumps({"run": "echo l"}), encoding="utf-8")
    (tmp_path / "broken.byk.json").write_text("not json", encoding="utf-8")
    (tmp_path / "bad.name.byk.json").write_text("{}", encoding="utf-8")

    r = runner.run("__api__", "file", "--list", cwd=str(tmp_path))
    assert r.returncode == 0, r.stderr
    workdir = tmp_path.resolve()
    assert json.loads(r.stdout) == {
        "files": [
            {
                "key": "@@g",
                "scope": "global",
                "path": str(global_file),
                "dir": str(runner.byk_dir / "alias"),
            },
            {
                "key": "@broken",
                "scope": "local",
                "path": str(workdir / "broken.byk.json"),
                "dir": str(workdir),
            },
            {
                "key": "@l",
                "scope": "local",
                "path": str(workdir / "l.byk.json"),
                "dir": str(workdir),
            },
        ]
    }


def test_api_file_list_empty_does_not_create_dirs(runner, tmp_path):
    r = runner.run("__api__", "file", "--list", cwd=str(tmp_path))
    assert r.returncode == 0, r.stderr
    assert json.loads(r.stdout) == {"files": []}
    assert not runner.byk_dir.exists()


def test_api_file_list_rejects_key(runner):
    r = runner.run("__api__", "file", "--list", "@ops")
    assert r.returncode == 1
    assert "too many arguments" in r.stderr


def test_api_file_rejects_unknown_option(runner):
    r = runner.run("__api__", "file", "--nope")
    assert r.returncode == 1
    assert 'unknown option "--nope"' in r.stderr
    assert "--list" in r.stderr


def test_api_file_matches_filecmd_path(runner, tmp_path):
    """file 的 path 与 @..path 输出的绝对路径一致"""
    r = runner.run("__api__", "file", "@ops", cwd=str(tmp_path))
    api_path = json.loads(r.stdout)["path"]
    r = runner.run("@ops..path", cwd=str(tmp_path))
    assert r.returncode == 0, r.stderr
    assert r.stdout == f"{api_path}\n"


def test_api_file_rejects_alias_key(runner, tmp_path):
    """file 只接受文件 key；带点的别名 key 不属于文件 namespace"""
    r = runner.run("__api__", "file", "@p.build", cwd=str(tmp_path))
    assert r.returncode == 1
    assert "Invalid file name" in r.stderr
    assert r.stdout == ""


def test_api_file_rejects_missing_prefix(runner):
    r = runner.run("__api__", "file", "ops")
    assert r.returncode == 1
    assert "Invalid file key" in r.stderr


def test_api_file_requires_key(runner):
    r = runner.run("__api__", "file")
    assert r.returncode == 1
    assert "missing <file-key>" in r.stderr


def test_api_file_rejects_extra_args(runner):
    r = runner.run("__api__", "file", "@ops", "extra")
    assert r.returncode == 1
    assert "too many arguments" in r.stderr


def test_api_expand_vars(runner, tmp_path):
    """expand 展开 $var 与内置变量，且不校验别名结构"""
    conf = tmp_path / "conf.json"
    conf.write_text(
        json.dumps(
            {
                "$var": {"TOOLS": "${byk_dir}/tools", "DEPLOY": "${TOOLS}/deploy"},
                "build": {
                    "$cmd": "cd ${file_dir} && ${DEPLOY} --work ${work_dir}",
                    "$unknown": "${nope}",
                },
                "not_an_alias": {"$cmd": 123},
            }
        ),
        encoding="utf-8",
    )
    r = runner.run("__api__", "expand", str(conf), cwd=str(tmp_path))
    assert r.returncode == 0, r.stderr
    workdir = tmp_path.resolve()
    payload = json.loads(r.stdout)
    assert "$var" not in payload
    assert payload["build"]["$cmd"] == (
        f"cd {workdir} && {runner.byk_dir}/tools/deploy --work {workdir}"
    )
    assert payload["build"]["$unknown"] == "${nope}"
    assert payload["not_an_alias"] == {"$cmd": 123}
    assert r.stdout.endswith("\n")


def test_api_expand_builtins_without_var(runner, tmp_path):
    """没有 $var 时内置变量照样展开"""
    conf = tmp_path / "plain.json"
    conf.write_text(
        '{"root":"${byk_dir}","work":"${work_dir}","here":"${file_dir}"}',
        encoding="utf-8",
    )
    r = runner.run("__api__", "expand", str(conf), cwd=str(tmp_path))
    assert r.returncode == 0, r.stderr
    assert json.loads(r.stdout) == {
        "root": str(runner.byk_dir),
        "work": str(tmp_path.resolve()),
        "here": str(tmp_path.resolve()),
    }


def test_api_expand_array_root(runner, tmp_path):
    conf = tmp_path / "list.json"
    conf.write_text('["${file_dir}/a", {"b":"${byk_dir}"}]', encoding="utf-8")
    r = runner.run("__api__", "expand", str(conf), cwd=str(tmp_path))
    assert r.returncode == 0, r.stderr
    workdir = tmp_path.resolve()
    assert json.loads(r.stdout) == [f"{workdir}/a", {"b": str(runner.byk_dir)}]


def test_api_expand_relative_path(runner, tmp_path):
    """相对路径按 cwd 解析，file_dir 仍是一个绝对目录"""
    sub = tmp_path / "sub"
    sub.mkdir()
    (sub / "conf.json").write_text('{"here":"${file_dir}"}', encoding="utf-8")
    r = runner.run("__api__", "expand", "sub/conf.json", cwd=str(tmp_path))
    assert r.returncode == 0, r.stderr
    assert json.loads(r.stdout)["here"] == str(tmp_path.resolve() / "sub")


def test_api_expand_warns_without_failing(runner, tmp_path):
    conf = tmp_path / "conf.json"
    conf.write_text(
        '{"$var":{"bad-name":"x"},"cmd":"echo ${byk_dir}"}', encoding="utf-8"
    )
    r = runner.run("__api__", "expand", str(conf), cwd=str(tmp_path))
    assert r.returncode == 0, r.stderr
    assert "Warning:" in r.stderr
    assert "bad-name" in r.stderr
    payload = json.loads(r.stdout)
    assert "$var" not in payload
    assert payload["cmd"] == f"echo {runner.byk_dir}"


def test_api_expand_rejects_invalid_json(runner, tmp_path):
    conf = tmp_path / "bad.json"
    conf.write_text("{not json", encoding="utf-8")
    r = runner.run("__api__", "expand", str(conf), cwd=str(tmp_path))
    assert r.returncode == 1
    assert "invalid JSON in" in r.stderr
    assert r.stdout == ""


def test_api_expand_rejects_directory(runner, tmp_path):
    r = runner.run("__api__", "expand", str(tmp_path))
    assert r.returncode == 1
    assert "refusing to read directory" in r.stderr


def test_api_expand_reports_missing_file(runner, tmp_path):
    r = runner.run("__api__", "expand", str(tmp_path / "nope.json"))
    assert r.returncode == 1
    assert "file not found" in r.stderr


def test_api_expand_treats_at_prefix_as_path(runner, tmp_path):
    """@ 前缀不会被当作文件 key"""
    r = runner.run("__api__", "expand", "@ops", cwd=str(tmp_path))
    assert r.returncode == 1
    assert "file not found" in r.stderr


def test_api_expand_rejects_missing_or_extra_args(runner, tmp_path):
    r = runner.run("__api__", "expand")
    assert r.returncode == 1
    assert "missing <path>" in r.stderr

    r = runner.run("__api__", "expand", "a.json", "b.json", cwd=str(tmp_path))
    assert r.returncode == 1
    assert "too many arguments" in r.stderr


def test_api_paths_single_value_is_bare(runner, tmp_path):
    """paths <name> 只输出裸字符串：无引号、无转义、结尾一个换行，可直接进 $()"""
    r = runner.run("__api__", "paths", "root")
    assert r.returncode == 0, r.stderr
    assert r.stdout == f"{runner.byk_dir}\n"

    r = runner.run("__api__", "paths", "alias")
    assert r.returncode == 0, r.stderr
    assert r.stdout == f"{runner.byk_dir / 'alias'}\n"

    r = runner.run("__api__", "paths", "workdir", cwd=str(tmp_path))
    assert r.returncode == 0, r.stderr
    assert r.stdout == f"{tmp_path.resolve()}\n"


def test_api_paths_does_not_create_dirs(runner):
    """paths 只回答位置，不顺手创建目录"""
    r = runner.run("__api__", "paths")
    assert r.returncode == 0, r.stderr
    assert not runner.byk_dir.exists()


def test_api_paths_rejects_unknown_name(runner):
    r = runner.run("__api__", "paths", "bin")
    assert r.returncode == 1
    assert 'unknown name "bin"' in r.stderr
    assert "root, alias, workdir" in r.stderr
    assert r.stdout == ""


def test_api_paths_rejects_extra_args(runner):
    r = runner.run("__api__", "paths", "root", "alias")
    assert r.returncode == 1
    assert "too many arguments" in r.stderr


def test_api_complete_lists_aliases(runner):
    """complete 输出候选，一行一个；空输入不混入文件作用域"""
    runner.prepare_alias_json("r", {"alpha": "echo a", "beta": "echo b"})
    r = runner.run("__api__", "complete", "")
    assert r.returncode == 0, r.stderr
    lines = r.stdout.split("\n")
    assert "alpha" in lines
    assert "beta" in lines
    assert not any(line.startswith("@") for line in lines)


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
        "@@ops..set\tCreate or update an alias",
        "@@ops..remove\tRemove an alias or group",
        "@@ops..json\tShow the alias file JSON",
        "@@ops..path\tPrint the absolute path",
        "@@ops..rename\tRename the alias file",
        "@@ops..clone\tCopy a local or remote alias file into this file",
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


def test_api_complete_file_keys_include_source_path(runner):
    """文件作用域候选附带别名文件路径，家目录显示为 ~"""
    runner.prepare_alias_json("ops", {"build": "echo hi"})
    r = runner.run("__api__", "complete", "--with-descriptions", "@")
    assert r.returncode == 0, r.stderr
    assert "@@ops\t~/.byk/alias/ops.byk.json" in r.stdout.splitlines()


def test_api_complete_local_file_key_includes_absolute_path(runner, tmp_path):
    """当前目录的文件 key 附带绝对路径"""
    (tmp_path / "p.byk.json").write_text(
        json.dumps({"build": "echo hi"}), encoding="utf-8"
    )
    r = runner.run(
        "__api__", "complete", "--with-descriptions", "@p", cwd=str(tmp_path)
    )
    assert r.returncode == 0, r.stderr
    assert f"@p\t{tmp_path}/p.byk.json" in r.stdout.splitlines()


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
