"""E2E —— 别名保留变量展开（$python / $venv / $byk）"""


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


def test_add_pip_no_target_shows_usage(runner):
    """byk add pip 无 target → 用法帮助，退出码 0"""
    r = runner.run("add", "pip")
    assert r.returncode == 0
    assert "byk add pip" in r.stdout


def test_add_pip_file_not_found(runner):
    """目标文件不存在 → 报错，退出码非 0"""
    r = runner.run("add", "pip", "@nope")
    assert r.returncode != 0
    assert "not found" in r.stderr


def test_add_pip_no_declaration_is_noop(runner):
    """文件无 $pip → 友好 no-op，且不触发 venv 创建（无交互卡住）"""
    runner.prepare_alias_file("nodeps", '{"hi": "echo hello"}')
    r = runner.run("add", "pip", "@@nodeps")
    assert r.returncode == 0
    assert "no $pip" in r.stdout


def test_add_pip_path_form(runner):
    """路径形态：任意位置的 .byk.json 都能作为 target"""
    path = runner.prepare_alias_file("nodeps", '{"hi": "echo hello"}')
    r = runner.run("add", "pip", str(path))
    assert r.returncode == 0
    assert "no $pip" in r.stdout


def test_add_unexpected_target_errors(runner):
    """comp 等功能不接受额外位置参数"""
    r = runner.run("add", "comp", "extra")
    assert r.returncode != 0
    assert "unexpected argument" in r.stderr


def test_args_tree_completion(runner):
    """$args 补全树：__complete 全链路（扫描→合并→补全）"""
    runner.prepare_alias_file(
        "args",
        '{"deploy": {"$cmd": "deploy {env}", "$args": {"$": ["prod", "staging"], "$options": ["--dry-run"]}}}',
    )
    r = runner.run("__complete", "deploy", "")
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
    r = runner.run("__complete", "deploy", "build", "")
    assert r.returncode == 0
    assert "debug" in r.stdout
    assert "release" in r.stdout


def test_args_tree_excludes_consumed_option(runner):
    """$options 已选过的选项在后续补全中排除"""
    runner.prepare_alias_file(
        "args",
        '{"deploy": {"$cmd": "deploy", "$args": {"$options": ["--dry-run", "--verbose"], "$": ["prod"]}}}',
    )
    r = runner.run("__complete", "deploy", "--dry-run", "")
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


def test_add_assets_single_flat_and_runs(runner):
    """$assets 单条目（全局文件+cwd 工作目录）：平铺 + $assets 变量执行"""
    work = runner.home / "work"
    work.mkdir(parents=True, exist_ok=True)
    runner.prepare_alias_file("ast", "{}")  # 先建目录
    src = runner.byk_dir / "alias" / "vendor.sh"
    src.write_text("#!/bin/sh\necho asset-ok\n")
    runner.prepare_alias_file(
        "ast",
        '{"$assets": {"tool.sh": "[exe] ' + str(src) + '"},'
        ' "run": {"$cmd": "$assets/tool.sh"}}',
    )
    r = runner.run("add", "assets", "@@ast", cwd=str(work))
    assert r.returncode == 0, r.stderr
    dest = work / "tool.sh"
    assert dest.is_file(), "单条目应平铺到工作目录"
    r2 = runner.run("run", cwd=str(work))
    assert r2.returncode == 0, r2.stderr
    assert "asset-ok" in r2.stdout


def test_add_assets_multi_container(runner):
    """$assets 多条目缺省根：自动进 assets/ 容器"""
    work = runner.home / "work2"
    work.mkdir(parents=True, exist_ok=True)
    runner.prepare_alias_file("ast2", "{}")  # 先建目录
    src = runner.byk_dir / "alias" / "vendor.sh"
    src.write_text("#!/bin/sh\necho ok\n")
    runner.prepare_alias_file(
        "ast2",
        '{"$assets": {"a.sh": "' + str(src) + '", "b.sh": "' + str(src) + '"}}',
    )
    r = runner.run("add", "assets", "@@ast2", cwd=str(work))
    assert r.returncode == 0, r.stderr
    assert (work / "assets" / "a.sh").is_file()
    assert (work / "assets" / "b.sh").is_file()


def test_add_assets_no_declaration_is_noop(runner):
    """文件无 $assets → 友好 no-op，不创建任何目录"""
    runner.prepare_alias_file("nodeps", '{"hi": "echo hello"}')
    r = runner.run("add", "assets", "@@nodeps")
    assert r.returncode == 0
    assert "no $assets" in r.stdout


def test_add_assets_file_not_found(runner):
    """目标文件不存在 → 报错，退出码非 0"""
    r = runner.run("add", "assets", "@nope")
    assert r.returncode != 0
    assert "not found" in r.stderr


def test_add_assets_skips_platform(runner):
    """条目平台映射不含当前平台 → nothing to download"""
    runner.prepare_alias_file(
        "ast3",
        '{"$assets": {"tool": {"nonexistent-platform": "https://x/tool"}}}',
    )
    r = runner.run("add", "assets", "@@ast3")
    assert r.returncode == 0
    assert "nothing to download" in r.stdout


def test_assets_var_without_declaration_errors(runner):
    """文件未声明 $assets 而命令用了 $assets → 明确报错"""
    runner.prepare_alias_file("noast", '{"run": {"$cmd": "$assets/tool"}}')
    r = runner.run("run")
    assert r.returncode != 0
    assert "$assets" in r.stderr
