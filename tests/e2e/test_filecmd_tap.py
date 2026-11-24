"""`@@@tap` 系统文件：把仓库装进 ~/.byk/tap，并验证 clone --tap 不需要改。

`add` 只走 `git clone`；这里用假 git 覆盖参数与失败路径，另有一条用真 git 的本地克隆。
"""

import json
import os
import shutil
import subprocess
import sys

import pytest

GIT_SCRIPT = """#!{python}
import json, os, sys

with open({spec!r}, encoding="utf-8") as handle:
    spec = json.load(handle)

args = sys.argv[1:]
with open({log!r}, "w", encoding="utf-8") as handle:
    handle.write("\\n".join(args) + "\\n")

if spec["exit"]:
    sys.stderr.write(spec["stderr"])
    sys.exit(spec["exit"])

dest = args[-1]
os.makedirs(dest, exist_ok=True)
os.makedirs(os.path.join(dest, ".git"), exist_ok=True)
for name, text in spec["files"].items():
    path = os.path.join(dest, name)
    os.makedirs(os.path.dirname(path), exist_ok=True)
    with open(path, "w", encoding="utf-8") as handle:
        handle.write(text)
"""


def write_script(path, body):
    path.write_text(body, encoding="utf-8")
    path.chmod(0o755)


def fake_git(tmp_path, *, files=None, exit_code=0, stderr=""):
    """造一个假 git，返回 (env, log)；log 不存在说明 git 没被调用。"""
    bin_dir = tmp_path / "bin"
    bin_dir.mkdir()
    spec_path = tmp_path / "git-spec.json"
    spec_path.write_text(
        json.dumps({"files": files or {}, "exit": exit_code, "stderr": stderr}),
        encoding="utf-8",
    )
    log = tmp_path / "git-args"
    write_script(
        bin_dir / "git",
        GIT_SCRIPT.format(python=sys.executable, spec=str(spec_path), log=str(log)),
    )
    return {"PATH": f"{bin_dir}{os.pathsep}{os.environ['PATH']}"}, log


TAP_REPO = {"byk.tap.json": '{"build": "aliases/build.byk.json"}'}


def read_index(runner):
    path = runner.byk_dir / "tap" / "index.json"
    return json.loads(path.read_text(encoding="utf-8"))


def tap_entries(runner):
    if not (runner.byk_dir / "tap").exists():
        return []
    return sorted(path.name for path in (runner.byk_dir / "tap").iterdir())


def add_via_git(runner, tmp_path, **overrides):
    files = overrides.pop("files", TAP_REPO)
    env, log = fake_git(tmp_path, files=files, **overrides)
    return runner.run("@@@tap", "add", "aaa/bb", env=env), log


def test_add_uses_a_shallow_git_clone(runner, tmp_path):
    result, log = add_via_git(runner, tmp_path)

    assert result.returncode == 0, result.stderr
    repo = runner.byk_dir / "tap" / "aaa" / "bb"
    assert (repo / "byk.tap.json").is_file()
    assert (repo / ".git").is_dir(), "装进来的是真仓库"
    assert read_index(runner) == {"aaa/bb": {"origin": "github", "ref": "HEAD"}}

    args = log.read_text(encoding="utf-8").splitlines()
    assert args[0] == "clone"
    assert "--depth" in args and args[args.index("--depth") + 1] == "1"
    assert args[-2] == "https://github.com/aaa/bb.git"


def test_add_accepts_any_git_url(runner, tmp_path):
    env, log = fake_git(tmp_path, files=TAP_REPO)

    result = runner.run(
        "@@@tap", "add", "my/tools", "ssh://git@git.example.com/my/tools.git", env=env
    )

    assert result.returncode == 0, result.stderr
    args = log.read_text(encoding="utf-8").splitlines()
    assert args[-2] == "ssh://git@git.example.com/my/tools.git", "URL 原样交给 git"
    assert "--" in args, "URL 必须落在 `--` 之后，不能被当成选项"
    assert "https://github.com/my/tools.git" not in args, "有自定义地址就不该再拼 GitHub"
    assert read_index(runner) == {
        "my/tools": {
            "origin": "git.example.com",
            "ref": "HEAD",
            "url": "ssh://git@git.example.com/my/tools.git",
        }
    }


def test_add_clones_a_local_git_repository(runner, tmp_path):
    """本地路径也能装：不设协议白名单，交给 git 就行。"""
    if shutil.which("git") is None:
        pytest.skip("需要本机 git")

    source = tmp_path / "source"
    source.mkdir()
    (source / "byk.tap.json").write_text('{"build": "aliases/build.byk.json"}')
    for argv in (
        ["init", "-q", str(source)],
        ["-C", str(source), "add", "."],
        [
            "-C",
            str(source),
            "-c",
            "user.email=t@example.com",
            "-c",
            "user.name=t",
            "commit",
            "-qm",
            "init",
        ],
    ):
        subprocess.run(["git", *argv], check=True, capture_output=True)

    result = runner.run("@@@tap", "add", "local/scratch", str(source))

    assert result.returncode == 0, result.stderr
    repo = runner.byk_dir / "tap" / "local" / "scratch"
    assert (repo / "byk.tap.json").is_file()
    assert (repo / ".git").is_dir()
    index = read_index(runner)
    assert index["local/scratch"]["origin"] == "local"
    assert index["local/scratch"]["url"] == str(source)


def test_add_passes_the_pinned_ref_to_git(runner, tmp_path):
    env, log = fake_git(tmp_path, files=TAP_REPO)

    result = runner.run("@@@tap", "add", "aaa/bb@v1", env=env)

    assert result.returncode == 0, result.stderr
    args = log.read_text(encoding="utf-8").splitlines()
    assert "--branch" in args and args[args.index("--branch") + 1] == "v1"
    assert read_index(runner) == {"aaa/bb": {"origin": "github", "ref": "v1"}}


def test_add_requires_git(runner, tmp_path):
    empty = tmp_path / "empty"
    empty.mkdir()

    for argv in (["aaa/bb"], ["aaa/bb", "https://git.example.com/aaa/bb.git"]):
        result = runner.run("@@@tap", "add", *argv, env={"PATH": str(empty)})

        assert result.returncode == 1
        assert "Installing a tap repository requires git" in result.stderr
        assert tap_entries(runner) == []


def test_add_reports_a_missing_repository(runner, tmp_path):
    env, _ = fake_git(tmp_path, exit_code=128, stderr="fatal: repository not found")

    result = runner.run("@@@tap", "add", "aaa/bb", env=env)

    assert result.returncode == 1
    assert "Failed to clone aaa/bb@HEAD" in result.stderr
    assert "fatal: repository not found" in result.stderr
    assert "check the user, repository, and ref" in result.stderr
    assert tap_entries(runner) == []


def test_add_suggests_a_proxy_for_transport_failures(runner, tmp_path):
    env, _ = fake_git(
        tmp_path,
        exit_code=128,
        stderr="fatal: unable to access 'https://github.com/aaa/bb.git/': CONNECT tunnel failed",
    )

    result = runner.run("@@@tap", "add", "aaa/bb", env=env)

    assert result.returncode == 1
    assert "set BYK_PROXY and retry" in result.stderr


def test_list_shows_installed_repositories(runner, tmp_path):
    result, _ = add_via_git(runner, tmp_path)
    assert result.returncode == 0

    listed = runner.run("@@@tap", "list")

    assert listed.returncode == 0
    assert "aaa/bb" in listed.stdout
    assert "github" in listed.stdout
    assert "HEAD" in listed.stdout
    assert "ok" in listed.stdout


def test_bare_call_equals_list(runner, tmp_path):
    result, _ = add_via_git(runner, tmp_path)
    assert result.returncode == 0

    bare = runner.run("@@@tap")
    listed = runner.run("@@@tap", "list")

    assert bare.returncode == 0
    assert bare.stdout == listed.stdout


def test_list_marks_a_hand_deleted_directory(runner, tmp_path):
    result, _ = add_via_git(runner, tmp_path)
    assert result.returncode == 0
    shutil.rmtree(runner.byk_dir / "tap" / "aaa" / "bb")

    listed = runner.run("@@@tap", "list")

    assert "aaa/bb" in listed.stdout
    assert "missing" in listed.stdout


def test_remove_deletes_the_directory_and_the_entry(runner, tmp_path):
    result, _ = add_via_git(runner, tmp_path)
    assert result.returncode == 0

    removed = runner.run("@@@tap", "remove", "aaa/bb")

    assert removed.returncode == 0, removed.stderr
    assert not (runner.byk_dir / "tap" / "aaa").exists()
    assert read_index(runner) == {}


def test_list_and_remove_work_without_git(runner, tmp_path):
    result, _ = add_via_git(runner, tmp_path)
    assert result.returncode == 0

    empty = tmp_path / "empty"
    empty.mkdir()
    env = {"PATH": str(empty)}

    listed = runner.run("@@@tap", "list", env=env)
    assert listed.returncode == 0
    assert "aaa/bb" in listed.stdout

    removed = runner.run("@@@tap", "remove", "aaa/bb", env=env)
    assert removed.returncode == 0, removed.stderr
    assert not (runner.byk_dir / "tap" / "aaa").exists()


def test_remove_refuses_a_repository_outside_the_index(runner):
    manual = runner.byk_dir / "tap" / "aaa" / "bb"
    manual.mkdir(parents=True)

    result = runner.run("@@@tap", "remove", "aaa/bb")

    assert result.returncode == 1
    assert "Tap repository is not installed" in result.stderr
    assert "Installed: (none)" in result.stderr
    assert manual.is_dir()


def test_add_refuses_to_overwrite_an_existing_directory(runner, tmp_path):
    env, log = fake_git(tmp_path, files=TAP_REPO)
    (runner.byk_dir / "tap" / "aaa" / "bb").mkdir(parents=True)

    result = runner.run("@@@tap", "add", "aaa/bb", env=env)

    assert result.returncode == 1
    assert "Tap repository already exists" in result.stderr
    assert not log.exists(), "目录已存在就什么都不该跑"


def test_add_rolls_back_a_repository_without_an_index(runner, tmp_path):
    env, _ = fake_git(tmp_path, files={"README.md": "not a tap repo"})

    result = runner.run("@@@tap", "add", "aaa/bb", env=env)

    assert result.returncode == 1
    assert "Not a tap repository" in result.stderr
    assert "byk.tap.json" in result.stderr
    assert tap_entries(runner) == [], "坏仓库不能留下任何东西"


def test_added_repository_feeds_clone_tap(runner, tmp_path):
    """装好的仓库直接能被 clone --tap 用上——clone 侧没有为索引改过一行。"""
    env, _ = fake_git(
        tmp_path,
        files={
            "byk.tap.json": '{"build": "aliases/build.byk.json"}',
            "aliases/build.byk.json": '{"build": "echo hi"}',
        },
    )
    assert runner.run("@@@tap", "add", "aaa/bb", env=env).returncode == 0

    result = runner.run("@@ops", "clone", "--tap", "aaa/bb/build")

    assert result.returncode == 0, result.stderr
    assert (runner.byk_dir / "alias" / "ops.byk.json").read_text(
        encoding="utf-8"
    ) == '{"build": "echo hi"}'


def test_add_rejects_a_bad_spec_without_network(runner, tmp_path):
    env, log = fake_git(tmp_path, files=TAP_REPO)

    result = runner.run("@@@tap", "add", "aaa", env=env)

    assert result.returncode == 1
    assert "Tap repository must be <user>/<repo>" in result.stderr
    assert not log.exists()


def test_add_rejects_a_source_that_looks_like_an_option(runner, tmp_path):
    env, log = fake_git(tmp_path, files=TAP_REPO)

    result = runner.run("@@@tap", "add", "aaa/bb", "--upload-pack=sh", env=env)

    assert result.returncode == 1
    assert "Invalid source" in result.stderr
    assert not log.exists()


def test_help_lists_the_verbs(runner):
    result = runner.run("@@@tap", "-h")

    assert result.returncode == 0
    assert "Usage: byk @@@tap" in result.stdout
    for verb in ("list", "add", "remove"):
        assert verb in result.stdout
