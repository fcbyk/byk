"""bash 补全脚本的活性验证：在 bash 里加载脚本，直接喂 COMP_WORDS / COMP_CWORD。

Git Bash 的 COMP_WORDBREAKS 含 @，`local@demo` 会被拆成 local / @ / demo，
而且 readline 只从最后一个 @ 开始替换——所以 COMPREPLY 里要摘掉重复的前缀。
Linux / macOS 上 @ 不是分隔符，整词进整词出。
"""

import os
import shutil
import subprocess

import pytest

SCRIPT = """set -u
export HOME='__HOME__'
cd '__CWD__'
eval "$(byk __api__ completion bash)"

run() {
    id=$1
    line=$2
    shift 2
    COMP_LINE=$line
    COMP_POINT=${#line}
    COMP_WORDS=("$@")
    COMP_CWORD=$(( $# - 1 ))
    _byk_completion
    printf '%s=%s\\n' "$id" "${COMPREPLY[*]}"
}

# Git Bash：@ 也是词分隔符
[[ $COMP_WORDBREAKS == *@* ]] || COMP_WORDBREAKS+="@"
run split_key 'byk local@' byk local @ ''
run split_key_no_empty 'byk local@' byk local @
run split_scope_prefix 'byk s@' byk s @ ''
run split_verbs 'byk local@demo ' byk local @ demo ''
run split_verb_prefix 'byk local@demo se' byk local @ demo se
run split_set_keys 'byk local@demo set ' byk local @ demo set ''
run split_sys 'byk sys@config ' byk sys @ config ''
run split_flags 'byk --' byk --
run split_alias_leaf 'byk hello ' byk hello ''

# Linux / macOS：@ 不切词
COMP_WORDBREAKS=${COMP_WORDBREAKS//@/}
run intact_key 'byk local@' byk 'local@'
run intact_verbs 'byk local@demo ' byk 'local@demo' ''
run intact_set_keys 'byk local@demo set ' byk 'local@demo' set ''
"""

VERBS = "init destroy clear set remove json path rename clone link deps"


def run_cases(runner, tmp_path):
    if shutil.which("bash") is None:
        pytest.skip("bash not installed")

    (tmp_path / "demo.byk.json").write_text(
        '{"build":"echo b","web":{"deploy":"echo x"}}', encoding="utf-8"
    )
    script = tmp_path / "check.sh"
    script.write_text(
        SCRIPT.replace("__HOME__", str(runner.home)).replace("__CWD__", str(tmp_path)),
        encoding="utf-8",
    )

    env = {
        **os.environ,
        "HOME": str(runner.home),
        "PATH": f"{runner.binary.parent}{os.pathsep}{os.environ.get('PATH', '')}",
    }
    result = subprocess.run(
        ["bash", "--noprofile", "--norc", str(script)],
        capture_output=True,
        text=True,
        env=env,
        cwd=str(tmp_path),
        timeout=120,
    )
    assert result.returncode == 0, result.stderr
    return dict(line.split("=", 1) for line in result.stdout.splitlines() if "=" in line)


def test_git_bash_shapes_are_rejoined_and_stripped(runner, tmp_path):
    cases = run_cases(runner, tmp_path)

    # readline 从 @ 开始替换，候选里的 `local` / `s` 前缀要摘掉
    assert cases["split_key"] == "@demo"
    assert cases["split_key_no_empty"] == "@demo"
    assert cases["split_scope_prefix"] == "@config @tap @shell"
    assert cases["split_verbs"] == VERBS
    assert cases["split_verb_prefix"] == "set"
    assert cases["split_set_keys"] == "build web web.deploy"
    assert cases["split_sys"] == "list get set unset"
    assert cases["split_flags"] == "--version --help --paths --check"
    # 别名只补第一个词：结尾空词位置没有候选
    assert cases["split_alias_leaf"] == ""


def test_intact_shapes_still_work(runner, tmp_path):
    cases = run_cases(runner, tmp_path)

    assert cases["intact_key"] == "local@demo"
    assert cases["intact_verbs"] == VERBS
    assert cases["intact_set_keys"] == "build web web.deploy"
