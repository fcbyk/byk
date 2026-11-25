"""`sys@tap` 系统文件：把仓库装进 ~/.byk/tap，并验证 clone --tap 不需要改。

`add` 只走 `git clone`；这里用假 git 覆盖参数与失败路径，另有一条用真 git 的本地克隆。
`update` 本质是 `fetch` + `reset --hard`，全部用真 git 的本地仓库验证——
假 git 只会证明「参数拼对了」，证明不了 checkout 真的跟上了上游。
"""

import json
import os
import shutil
import subprocess
import sys

import pytest

requires_git = pytest.mark.skipif(shutil.which("git") is None, reason="需要本机 git")

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
    return runner.run("sys@tap", "add", "aaa/bb", env=env), log


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
        "sys@tap", "add", "my/tools", "ssh://git@git.example.com/my/tools.git", env=env
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

    result = runner.run("sys@tap", "add", "local/scratch", str(source))

    assert result.returncode == 0, result.stderr
    repo = runner.byk_dir / "tap" / "local" / "scratch"
    assert (repo / "byk.tap.json").is_file()
    assert (repo / ".git").is_dir()
    index = read_index(runner)
    assert index["local/scratch"]["origin"] == "local"
    assert index["local/scratch"]["url"] == str(source)


def test_add_passes_the_pinned_ref_to_git(runner, tmp_path):
    env, log = fake_git(tmp_path, files=TAP_REPO)

    result = runner.run("sys@tap", "add", "aaa/bb@v1", env=env)

    assert result.returncode == 0, result.stderr
    args = log.read_text(encoding="utf-8").splitlines()
    assert "--branch" in args and args[args.index("--branch") + 1] == "v1"
    assert read_index(runner) == {"aaa/bb": {"origin": "github", "ref": "v1"}}


def test_add_requires_git(runner, tmp_path):
    empty = tmp_path / "empty"
    empty.mkdir()

    for argv in (["aaa/bb"], ["aaa/bb", "https://git.example.com/aaa/bb.git"]):
        result = runner.run("sys@tap", "add", *argv, env={"PATH": str(empty)})

        assert result.returncode == 1
        assert "Installing a tap repository requires git" in result.stderr
        assert tap_entries(runner) == []


def test_add_reports_a_missing_repository(runner, tmp_path):
    env, _ = fake_git(tmp_path, exit_code=128, stderr="fatal: repository not found")

    result = runner.run("sys@tap", "add", "aaa/bb", env=env)

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

    result = runner.run("sys@tap", "add", "aaa/bb", env=env)

    assert result.returncode == 1
    assert "set BYK_PROXY and retry" in result.stderr


def test_list_shows_installed_repositories(runner, tmp_path):
    result, _ = add_via_git(runner, tmp_path)
    assert result.returncode == 0

    listed = runner.run("sys@tap", "list")

    assert listed.returncode == 0
    assert "aaa/bb" in listed.stdout
    assert "github" in listed.stdout
    assert "HEAD" in listed.stdout
    assert "ok" in listed.stdout


def test_bare_call_equals_list(runner, tmp_path):
    result, _ = add_via_git(runner, tmp_path)
    assert result.returncode == 0

    bare = runner.run("sys@tap")
    listed = runner.run("sys@tap", "list")

    assert bare.returncode == 0
    assert bare.stdout == listed.stdout


def test_list_marks_a_hand_deleted_directory(runner, tmp_path):
    result, _ = add_via_git(runner, tmp_path)
    assert result.returncode == 0
    shutil.rmtree(runner.byk_dir / "tap" / "aaa" / "bb")

    listed = runner.run("sys@tap", "list")

    assert "aaa/bb" in listed.stdout
    assert "missing" in listed.stdout


def test_remove_deletes_the_directory_and_the_entry(runner, tmp_path):
    result, _ = add_via_git(runner, tmp_path)
    assert result.returncode == 0

    removed = runner.run("sys@tap", "remove", "aaa/bb")

    assert removed.returncode == 0, removed.stderr
    assert not (runner.byk_dir / "tap" / "aaa").exists()
    assert read_index(runner) == {}


def test_list_and_remove_work_without_git(runner, tmp_path):
    result, _ = add_via_git(runner, tmp_path)
    assert result.returncode == 0

    empty = tmp_path / "empty"
    empty.mkdir()
    env = {"PATH": str(empty)}

    listed = runner.run("sys@tap", "list", env=env)
    assert listed.returncode == 0
    assert "aaa/bb" in listed.stdout

    removed = runner.run("sys@tap", "remove", "aaa/bb", env=env)
    assert removed.returncode == 0, removed.stderr
    assert not (runner.byk_dir / "tap" / "aaa").exists()


def test_remove_refuses_a_repository_outside_the_index(runner):
    manual = runner.byk_dir / "tap" / "aaa" / "bb"
    manual.mkdir(parents=True)

    result = runner.run("sys@tap", "remove", "aaa/bb")

    assert result.returncode == 1
    assert "Tap repository is not installed" in result.stderr
    assert "Installed: (none)" in result.stderr
    assert manual.is_dir()


def test_add_refuses_to_overwrite_an_existing_directory(runner, tmp_path):
    env, log = fake_git(tmp_path, files=TAP_REPO)
    (runner.byk_dir / "tap" / "aaa" / "bb").mkdir(parents=True)

    result = runner.run("sys@tap", "add", "aaa/bb", env=env)

    assert result.returncode == 1
    assert "Tap repository already exists" in result.stderr
    assert not log.exists(), "目录已存在就什么都不该跑"


def test_add_rolls_back_a_repository_without_an_index(runner, tmp_path):
    env, _ = fake_git(tmp_path, files={"README.md": "not a tap repo"})

    result = runner.run("sys@tap", "add", "aaa/bb", env=env)

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
    assert runner.run("sys@tap", "add", "aaa/bb", env=env).returncode == 0

    result = runner.run("global@ops", "clone", "--tap", "aaa/bb/build")

    assert result.returncode == 0, result.stderr
    assert (runner.byk_dir / "alias" / "ops.byk.json").read_text(
        encoding="utf-8"
    ) == '{"build": "echo hi"}'


def test_add_rejects_a_bad_spec_without_network(runner, tmp_path):
    env, log = fake_git(tmp_path, files=TAP_REPO)

    result = runner.run("sys@tap", "add", "aaa", env=env)

    assert result.returncode == 1
    assert "Tap repository must be <user>/<repo>" in result.stderr
    assert not log.exists()


def test_add_rejects_a_source_that_looks_like_an_option(runner, tmp_path):
    env, log = fake_git(tmp_path, files=TAP_REPO)

    result = runner.run("sys@tap", "add", "aaa/bb", "--upload-pack=sh", env=env)

    assert result.returncode == 1
    assert "Invalid source" in result.stderr
    assert not log.exists()


def test_help_lists_the_verbs(runner):
    result = runner.run("sys@tap", "-h")

    assert result.returncode == 0
    assert "Usage: byk sys@tap" in result.stdout
    for verb in ("list", "add", "update", "remove"):
        assert verb in result.stdout


# ── update：把装好的仓库拉到上游当前位置 ──────────────────────────────


def git(*argv, cwd=None):
    subprocess.run(["git", *argv], check=True, capture_output=True, cwd=cwd)


def commit(source, files, message="change"):
    """改几份文件再提交；值写成 `None` 表示删掉那个文件。"""
    for name, text in files.items():
        path = source / name
        if text is None:
            path.unlink()
            continue
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text(text, encoding="utf-8")
    git("-c", "user.email=t@example.com", "-c", "user.name=t", "add", "-A", cwd=source)
    git(
        "-c",
        "user.email=t@example.com",
        "-c",
        "user.name=t",
        "commit",
        "-qm",
        message,
        cwd=source,
    )


def make_upstream(tmp_path, name="upstream", files=None):
    source = tmp_path / name
    source.mkdir()
    git("init", "-q", str(source))
    commit(source, files or TAP_REPO, "init")
    return source


def install(runner, tmp_path, key="local/scratch", name="upstream", files=None):
    source = make_upstream(tmp_path, name=name, files=files)
    result = runner.run("sys@tap", "add", key, str(source))
    assert result.returncode == 0, result.stderr
    return source


def tap_path(runner, key="local/scratch"):
    return runner.byk_dir / "tap" / key


@requires_git
def test_update_pulls_new_commits(runner, tmp_path):
    source = install(runner, tmp_path)
    commit(source, {"later.byk.json": '{"build": "echo hi"}'})

    result = runner.run("sys@tap", "update", "local/scratch")

    assert result.returncode == 0, result.stderr
    assert "Updated local/scratch" in result.stderr
    assert (tap_path(runner) / "later.byk.json").is_file(), "checkout 要跟上上游"
    assert read_index(runner)["local/scratch"]["ref"] == "HEAD", "更新不改账本"


@requires_git
def test_update_says_so_when_there_is_nothing_new(runner, tmp_path):
    install(runner, tmp_path)

    result = runner.run("sys@tap", "update", "local/scratch")

    assert result.returncode == 0, result.stderr
    assert "local/scratch is already up to date" in result.stderr


@requires_git
def test_update_follows_a_file_removed_upstream(runner, tmp_path):
    source = install(
        runner,
        tmp_path,
        files={"byk.tap.json": '{"build": "b.byk.json"}', "old.byk.json": "{}"},
    )
    commit(source, {"old.byk.json": None})

    assert runner.run("sys@tap", "update", "local/scratch").returncode == 0

    assert not (tap_path(runner) / "old.byk.json").exists(), "上游删掉的文件不能留下"


@requires_git
def test_update_follows_the_ref_the_index_recorded(runner, tmp_path):
    """命令行没写 `@v1` 也不会跑回默认分支：账本才是真相。"""
    source = make_upstream(tmp_path)
    git("tag", "v1", cwd=source)
    assert runner.run("sys@tap", "add", "local/scratch@v1", str(source)).returncode == 0
    commit(source, {"later.byk.json": '{"build": "echo hi"}'})

    result = runner.run("sys@tap", "update", "local/scratch")

    assert result.returncode == 0, result.stderr
    assert "already up to date" in result.stderr
    assert read_index(runner)["local/scratch"]["ref"] == "v1"
    assert not (tap_path(runner) / "later.byk.json").exists()


@requires_git
def test_update_all_walks_the_whole_index(runner, tmp_path):
    first = install(runner, tmp_path, key="local/one", name="one")
    second = install(runner, tmp_path, key="local/two", name="two")
    commit(first, {"a.byk.json": "{}"})
    commit(second, {"b.byk.json": "{}"})

    result = runner.run("sys@tap", "update")

    assert result.returncode == 0, result.stderr
    assert "Updated local/one" in result.stderr
    assert "Updated local/two" in result.stderr


@requires_git
def test_update_keeps_going_when_one_repository_fails(runner, tmp_path):
    good = install(runner, tmp_path, key="local/one", name="one")
    gone = install(runner, tmp_path, key="local/two", name="two")
    shutil.rmtree(gone)
    commit(good, {"a.byk.json": "{}"})

    result = runner.run("sys@tap", "update")

    assert result.returncode == 1
    assert "Updated local/one" in result.stderr, "一份失败不该拦下其余"
    assert "Failed to update local/two@HEAD" in result.stderr
    assert "Failed to update 1 of 2 tap repositories: local/two" in result.stderr


@requires_git
def test_update_rolls_back_a_checkout_without_an_index(runner, tmp_path):
    source = install(runner, tmp_path)
    commit(source, {"byk.tap.json": None, "other.json": "{}"})

    result = runner.run("sys@tap", "update", "local/scratch")

    assert result.returncode == 1
    assert "Not a tap repository" in result.stderr
    assert "the update was rolled back" in result.stderr
    assert (tap_path(runner) / "byk.tap.json").is_file(), "回滚要把索引放回去"


@requires_git
def test_update_refuses_a_checkout_with_local_changes(runner, tmp_path):
    source = install(runner, tmp_path)
    commit(source, {"upstream.byk.json": "{}"})
    local = tap_path(runner) / "byk.tap.json"
    local.write_text('{"build": "edited"}', encoding="utf-8")

    result = runner.run("sys@tap", "update", "local/scratch")

    assert result.returncode == 1
    assert "has local changes" in result.stderr
    assert local.read_text(encoding="utf-8") == '{"build": "edited"}', "本地改动不能被抹掉"


@requires_git
def test_update_reports_a_missing_checkout(runner, tmp_path):
    install(runner, tmp_path)
    shutil.rmtree(tap_path(runner))

    result = runner.run("sys@tap", "update", "local/scratch")

    assert result.returncode == 1
    assert "Tap repository is missing" in result.stderr


def test_update_requires_git(runner, tmp_path):
    repo = tap_path(runner, "aaa/bb")
    (repo / ".git").mkdir(parents=True)
    (repo / "byk.tap.json").write_text(TAP_REPO["byk.tap.json"], encoding="utf-8")
    (runner.byk_dir / "tap" / "index.json").write_text(
        json.dumps({"aaa/bb": {"origin": "github", "ref": "HEAD"}}), encoding="utf-8"
    )
    empty = tmp_path / "empty"
    empty.mkdir()

    result = runner.run("sys@tap", "update", "aaa/bb", env={"PATH": str(empty)})

    assert result.returncode == 1
    assert "Updating a tap repository requires git" in result.stderr


def test_update_refuses_a_repository_outside_the_index(runner):
    tap_path(runner, "aaa/bb").mkdir(parents=True)

    result = runner.run("sys@tap", "update", "aaa/bb")

    assert result.returncode == 1
    assert "Tap repository is not installed" in result.stderr
    assert "Installed: (none)" in result.stderr


def test_update_without_any_repository_is_not_an_error(runner):
    result = runner.run("sys@tap", "update")

    assert result.returncode == 0
    assert "No repositories installed" in result.stderr
    assert "byk sys@tap add aaa/bb" in result.stderr


def test_update_takes_at_most_one_repository(runner):
    result = runner.run("sys@tap", "update", "aaa/bb", "ccc/dd")

    assert result.returncode == 1
    assert "update takes one repository, or none" in result.stderr
