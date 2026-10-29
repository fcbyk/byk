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
