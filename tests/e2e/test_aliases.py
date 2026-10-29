"""E2E —— 别名保留变量展开（$python / $venv / $byk）"""

import json


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


def test_python_missing_venv_errors(runner):
    """venv 不存在时 $python 友好报错，退出码非 0，无 shell 乱码"""
    runner.prepare_alias_file("vars", '{"py": {"$cmd": "$python -V"}}')
    r = runner.run("py")
    assert r.returncode != 0
    assert "venv" in r.stderr.lower()


def test_add_unexpected_target_errors(runner):
    """comp 等功能不接受额外位置参数"""
    r = runner.run("add", "comp", "extra")
    assert r.returncode != 0
    assert "unexpected argument" in r.stderr


def test_args_tree_completion(runner):
    """$args 补全树：__api__ complete 全链路（扫描→合并→补全）"""
    runner.prepare_alias_file(
        "args",
        '{"deploy": {"$cmd": "deploy {env}", "$args": {"$": ["prod", "staging"], "$options": ["--dry-run"]}}}',
    )
    r = runner.run("__api__", "complete", "deploy", "")
    assert r.returncode == 0
    assert "prod" in r.stdout
    assert "staging" in r.stdout
    assert "--dry-run" in r.stdout


def test_args_tree_nested_completion(runner):
    """$args 嵌套子命令：下降一层后补全子层候选"""
    runner.prepare_alias_file(
        "args",
        '{"deploy": {"$cmd": "deploy", "$args": {"build": {"$": ["debug", "release"]}}}}',
    )
    r = runner.run("__api__", "complete", "deploy", "build", "")
    assert r.returncode == 0
    assert "debug" in r.stdout
    assert "release" in r.stdout


def test_args_tree_excludes_consumed_option(runner):
    """$options 已选过的选项在后续补全中排除"""
    runner.prepare_alias_file(
        "args",
        '{"deploy": {"$cmd": "deploy", "$args": {"$options": ["--dry-run", "--verbose"], "$": ["prod"]}}}',
    )
    r = runner.run("__api__", "complete", "deploy", "--dry-run", "")
    assert r.returncode == 0
    assert "--verbose" in r.stdout
    assert "prod" in r.stdout
    assert "--dry-run" not in r.stdout


def test_var_substitution_in_cmd(runner):
    """$var 文件级变量：阶段一替换后命令直接生效"""
    runner.prepare_alias_file(
        "vars",
        '{"$var": {"MSG": "hello-var"}, "say": {"$cmd": "echo $MSG"}}',
    )
    r = runner.run("say")
    assert r.returncode == 0
    assert "hello-var" in r.stdout


def test_assets_var_without_declaration_errors(runner):
    """文件未声明 $assets 而命令用了 $assets → 明确报错"""
    runner.prepare_alias_file("noast", '{"run": {"$cmd": "$assets/tool"}}')
    r = runner.run("run")
    assert r.returncode != 0
    assert "$assets" in r.stderr


# ==================== byk add <file>（插件安装） ====================


def test_install_plugin_writes_copy(runner):
    """$add → 装副本到全局（含 $add + 全部别名）+ 全局可解析"""
    work = runner.home / "work"
    work.mkdir(parents=True, exist_ok=True)
    (work / "tool.byk.json").write_text(
        '{"$add": {"pip": ["rich"]},'
        ' "tools": {"hi": {"$cmd": "echo hi"}},'
        ' "dev": "vite dev"}'
    )
    r = runner.run("add", "tool", cwd=str(work))
    assert r.returncode == 0, r.stderr

    dest = runner.byk_dir / "alias" / "tool.byk.json"
    assert dest.is_file()
    data = json.loads(dest.read_text())
    assert "$add" in data
    assert data["tools"]["hi"]["$cmd"] == "echo hi"
    assert data["dev"] == "vite dev"

    r2 = runner.run("tools.hi")
    assert r2.returncode == 0
    assert "hi" in r2.stdout


def test_install_plugin_assets_only(runner):
    """$add 只有 assets（无别名）：仍装 + 下载资源 + 写副本"""
    work = runner.home / "work2"
    work.mkdir(parents=True, exist_ok=True)
    runner.prepare_alias_file("vendor", "{}")  # 先建全局 alias 目录
    src = runner.byk_dir / "alias" / "vendor.sh"
    src.write_text("#!/bin/sh\necho ok\n")
    (work / "asset.byk.json").write_text(
        '{"$add": {"assets": {"$to": "$byk/tools/asset", "tool.sh": "[exe] ' + str(src) + '"}}}'
    )
    r = runner.run("add", "asset", cwd=str(work))
    assert r.returncode == 0, r.stderr
    # 资源下载到 $to
    assert (runner.byk_dir / "tools" / "asset" / "tool.sh").is_file()
    # 副本写入（保留 $add 声明，无别名）
    dest = runner.byk_dir / "alias" / "asset.byk.json"
    assert dest.is_file()
    data = json.loads(dest.read_text())
    assert "$add" in data


def test_install_plugin_without_flag_errors(runner):
    """无 $add → 报错，不装依赖不写副本"""
    work = runner.home / "work3"
    work.mkdir(parents=True, exist_ok=True)
    (work / "empty.byk.json").write_text('{"x": "echo x"}')
    r = runner.run("add", "empty", cwd=str(work))
    assert r.returncode != 0
    assert "not a plugin" in r.stderr
    assert not (runner.byk_dir / "alias" / "empty.byk.json").exists()


def test_install_alias_file_not_found(runner):
    """<file> 文件不存在 → 报错"""
    r = runner.run("add", "nope")
    assert r.returncode != 0
    assert "not found" in r.stderr


def test_install_alias_from_path(runner):
    """-f 本地路径：任意路径的 .byk.json 文件"""
    work = runner.home / "work4"
    work.mkdir(parents=True, exist_ok=True)
    sub = work / "sub"
    sub.mkdir(parents=True, exist_ok=True)
    (sub / "x.byk.json").write_text(
        '{"$add": {"pip": ["rich"]}, "hi": {"$cmd": "echo from-path"}}'
    )
    r = runner.run("add", "-f", str(sub / "x.byk.json"), cwd=str(work))
    assert r.returncode == 0, r.stderr
    assert (runner.byk_dir / "alias" / "x.byk.json").is_file()


def test_install_remote_no_path_uses_repo_name(runner):
    """远程 spec 缺 path（user/repo）→ 默认使用 repo.byk.json，不存在则报下载错误"""
    r = runner.run("add", "user/repo")
    assert r.returncode != 0
    assert "missing a file path" not in r.stderr