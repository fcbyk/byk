"""PowerShell 补全注册脚本的活性验证：在 pwsh 里加载脚本，用 TabExpansion2 走真实补全链。

装了 pwsh 才跑（brew 的 powershell 或官方 tar 包都行）；没有就 skip。
CI 的 ubuntu runner 自带 pwsh，这条链在那里照样覆盖。

PowerShell 这边覆盖全局选项与 byk 自己解释的参数（文件域动词、key、选项）。
"""

import os
import shutil
import subprocess

import pytest

# 用成员枚举代替 ForEach-Object，模板里就不必跟 PowerShell 的 `{}` 打交道
SCRIPT = """byk __api__ completion powershell | Out-String | Invoke-Expression
$line = '__LINE__'
$result = TabExpansion2 $line $line.Length
($result.CompletionMatches.__FIELD__ -join "`n")
"""


def run_in_pwsh(runner, body: str) -> str:
    pwsh = shutil.which("pwsh") or shutil.which("pwsh.exe")
    if pwsh is None:
        pytest.skip("pwsh not installed")

    env = {
        **os.environ,
        "HOME": str(runner.home),
        "PATH": f"{runner.binary.parent}{os.pathsep}{os.environ.get('PATH', '')}",
    }
    result = subprocess.run(
        [pwsh, "-NoProfile", "-NonInteractive", "-Command", body],
        capture_output=True,
        text=True,
        env=env,
        cwd=str(runner.home),
        timeout=120,
    )
    assert result.returncode == 0, result.stderr
    return result.stdout


def complete(runner, line: str, field: str = "CompletionText") -> list[str]:
    # PowerShell 单引号字符串里，引号本身写成两个单引号
    escaped = line.replace("'", "''")
    body = SCRIPT.replace("__LINE__", escaped).replace("__FIELD__", field)
    return [item for item in run_in_pwsh(runner, body).splitlines() if item]


def test_completes_the_first_word(runner):
    runner.prepare_alias_json(
        "demo", {"deploy": "echo d", "destroy": "echo x", "hello": "echo h"}
    )

    assert complete(runner, "byk de") == ["deploy", "destroy"]
    assert complete(runner, "byk ") == ["deploy", "destroy", "hello"]


def test_stops_after_the_first_word(runner):
    """别名只补第一个词：空词位置不给别名候选。

    PowerShell 对空结果会回退到文件名补全（壳的惯例，kubectl / gh 亦然），
    所以断言的是「别名不再出现」，不是「一个候选都没有」。
    """
    runner.prepare_alias_json("demo", {"hello": "echo h"})

    assert "hello" not in complete(runner, "byk hello ")


def test_completes_global_options(runner):
    assert complete(runner, "byk --c") == ["--check"]
    assert complete(runner, "byk --p") == ["--paths"]


def test_descriptions_land_in_tooltips(runner):
    assert complete(runner, "byk --h", field="ToolTip") == ["Print help"]


def test_completes_file_commands_after_the_key(runner):
    """`byk global@ops <TAB>` 补动词：5.1 传不了空参数，脚本改用 `--after-space`"""
    runner.prepare_alias_json("ops", {"build": "echo b", "web": {"deploy": "echo d"}})

    assert complete(runner, "byk global@ops ") == [
        "init",
        "destroy",
        "clear",
        "set",
        "remove",
        "json",
        "path",
        "rename",
        "clone",
        "link",
        "deps",
    ]
    assert complete(runner, "byk global@ops r") == ["remove", "rename"]
    assert "build" in complete(runner, "byk global@ops set ")


def test_completes_system_file_commands_after_the_name(runner):
    assert complete(runner, "byk special@config ") == ["list", "get", "set", "unset"]
