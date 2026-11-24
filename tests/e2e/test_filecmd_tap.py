"""`@@@tap` 系统文件：把仓库装进 ~/.byk/tap，并验证 clone --tap 不需要改。"""

import json
import os
import sys

import pytest


def install_fake_curl(tmp_path, files, status="200"):
    """假 curl：把 files 打成 tar.gz 写进 `-o` 指向的路径，并按 `-w` 打印状态码。

    返回 (env, args_path)；args_path 记录每次调用的参数，文件不存在说明没联网。
    """
    bin_dir = tmp_path / "bin"
    bin_dir.mkdir()
    args_path = tmp_path / "curl-args"
    spec_path = tmp_path / "archive-spec.json"
    spec_path.write_text(
        json.dumps({"files": files, "status": status}, ensure_ascii=False),
        encoding="utf-8",
    )

    script = bin_dir / "curl"
    script.write_text(
        f"""#!{sys.executable}
import io, json, sys, tarfile

with open({str(spec_path)!r}, encoding="utf-8") as handle:
    spec = json.load(handle)

args = sys.argv[1:]
with open({str(args_path)!r}, "w", encoding="utf-8") as log:
    log.write("\\n".join(args) + "\\n")

if spec["status"] != "200":
    sys.stdout.write(spec["status"])
    sys.exit(0)

archive = None
for index, arg in enumerate(args):
    if arg == "-o":
        archive = args[index + 1]

buffer = io.BytesIO()
with tarfile.open(fileobj=buffer, mode="w:gz") as tar:
    for name, text in spec["files"].items():
        data = text.encode("utf-8")
        info = tarfile.TarInfo(name)
        info.size = len(data)
        tar.addfile(info, io.BytesIO(data))

with open(archive, "wb") as handle:
    handle.write(buffer.getvalue())

sys.stdout.write("200")
""",
        encoding="utf-8",
    )
    script.chmod(0o755)

    env = {"PATH": f"{bin_dir}{os.pathsep}{os.environ['PATH']}"}
    return env, args_path


TAP_REPO = {"bb-HEAD/byk.tap.json": '{"build": "aliases/build.byk.json"}'}


def read_index(runner):
    path = runner.byk_dir / "tap" / "index.json"
    return json.loads(path.read_text(encoding="utf-8"))


def tap_entries(runner):
    if not (runner.byk_dir / "tap").exists():
        return []
    return sorted(path.name for path in (runner.byk_dir / "tap").iterdir())


def test_add_installs_and_records(runner, tmp_path):
    env, args_path = install_fake_curl(tmp_path, TAP_REPO)

    result = runner.run("@@@tap", "add", "aaa/bb", env=env)

    assert result.returncode == 0, result.stderr
    repo = runner.byk_dir / "tap" / "aaa" / "bb"
    assert (repo / "byk.tap.json").read_text(encoding="utf-8") == (
        '{"build": "aliases/build.byk.json"}'
    )
    assert read_index(runner) == {"aaa/bb": {"origin": "github", "ref": "HEAD"}}
    assert "https://codeload.github.com/aaa/bb/tar.gz/HEAD" in args_path.read_text(
        encoding="utf-8"
    )
    # 临时归档与解压目录都不能留下
    assert tap_entries(runner) == ["aaa", "index.json"]


def test_add_keeps_the_pinned_ref(runner, tmp_path):
    env, args_path = install_fake_curl(tmp_path, TAP_REPO)

    result = runner.run("@@@tap", "add", "aaa/bb@v1", env=env)

    assert result.returncode == 0, result.stderr
    assert read_index(runner) == {"aaa/bb": {"origin": "github", "ref": "v1"}}
    assert "https://codeload.github.com/aaa/bb/tar.gz/v1" in args_path.read_text(
        encoding="utf-8"
    )
    assert (runner.byk_dir / "tap" / "aaa" / "bb").is_dir()


def test_list_shows_installed_repositories(runner, tmp_path):
    env, _ = install_fake_curl(tmp_path, TAP_REPO)
    assert runner.run("@@@tap", "add", "aaa/bb", env=env).returncode == 0

    result = runner.run("@@@tap", "list")

    assert result.returncode == 0
    assert "aaa/bb" in result.stdout
    assert "github" in result.stdout
    assert "HEAD" in result.stdout
    assert "ok" in result.stdout


def test_bare_call_equals_list(runner, tmp_path):
    env, _ = install_fake_curl(tmp_path, TAP_REPO)
    assert runner.run("@@@tap", "add", "aaa/bb", env=env).returncode == 0

    bare = runner.run("@@@tap")
    listed = runner.run("@@@tap", "list")

    assert bare.returncode == 0
    assert bare.stdout == listed.stdout


def test_list_marks_a_hand_deleted_directory(runner, tmp_path):
    env, _ = install_fake_curl(tmp_path, TAP_REPO)
    assert runner.run("@@@tap", "add", "aaa/bb", env=env).returncode == 0
    import shutil

    shutil.rmtree(runner.byk_dir / "tap" / "aaa" / "bb")

    result = runner.run("@@@tap", "list")

    assert "aaa/bb" in result.stdout
    assert "missing" in result.stdout


def test_remove_deletes_the_directory_and_the_entry(runner, tmp_path):
    env, _ = install_fake_curl(tmp_path, TAP_REPO)
    assert runner.run("@@@tap", "add", "aaa/bb", env=env).returncode == 0

    result = runner.run("@@@tap", "remove", "aaa/bb")

    assert result.returncode == 0, result.stderr
    assert not (runner.byk_dir / "tap" / "aaa").exists()
    assert read_index(runner) == {}


def test_remove_refuses_a_repository_outside_the_index(runner):
    manual = runner.byk_dir / "tap" / "aaa" / "bb"
    manual.mkdir(parents=True)

    result = runner.run("@@@tap", "remove", "aaa/bb")

    assert result.returncode == 1
    assert "Tap repository is not installed" in result.stderr
    assert "Installed: (none)" in result.stderr
    assert manual.is_dir()


def test_add_refuses_to_overwrite_an_existing_directory(runner, tmp_path):
    env, args_path = install_fake_curl(tmp_path, TAP_REPO)
    (runner.byk_dir / "tap" / "aaa" / "bb").mkdir(parents=True)

    result = runner.run("@@@tap", "add", "aaa/bb", env=env)

    assert result.returncode == 1
    assert "Tap repository already exists" in result.stderr
    assert not args_path.exists(), "目录已存在就不该联网"


def test_add_reports_a_missing_repository(runner, tmp_path):
    env, _ = install_fake_curl(tmp_path, {}, status="404")

    result = runner.run("@@@tap", "add", "aaa/bb", env=env)

    assert result.returncode == 1
    assert "Tap repository not found" in result.stderr
    assert "HTTP 404" in result.stderr
    assert tap_entries(runner) == []


def test_add_rolls_back_a_repository_without_an_index(runner, tmp_path):
    env, _ = install_fake_curl(tmp_path, {"bb-HEAD/README.md": "not a tap repo"})

    result = runner.run("@@@tap", "add", "aaa/bb", env=env)

    assert result.returncode == 1
    assert "Not a tap repository" in result.stderr
    assert "byk.tap.json" in result.stderr
    assert tap_entries(runner) == [], "坏仓库不能留下任何东西"


def test_added_repository_feeds_clone_tap(runner, tmp_path):
    """装好的仓库直接能被 clone --tap 用上——clone 侧没有为索引改过一行。"""
    env, _ = install_fake_curl(
        tmp_path,
        {
            "bb-HEAD/byk.tap.json": '{"build": "aliases/build.byk.json"}',
            "bb-HEAD/aliases/build.byk.json": '{"build": "echo hi"}',
        },
    )
    assert runner.run("@@@tap", "add", "aaa/bb", env=env).returncode == 0

    result = runner.run("@@ops", "clone", "--tap", "aaa/bb/build")

    assert result.returncode == 0, result.stderr
    assert (runner.byk_dir / "alias" / "ops.byk.json").read_text(
        encoding="utf-8"
    ) == '{"build": "echo hi"}'


def test_add_rejects_a_bad_spec_without_network(runner, tmp_path):
    env, args_path = install_fake_curl(tmp_path, TAP_REPO)

    result = runner.run("@@@tap", "add", "aaa", env=env)

    assert result.returncode == 1
    assert "Tap repository must be <user>/<repo>" in result.stderr
    assert not args_path.exists()


def test_help_lists_the_verbs(runner):
    result = runner.run("@@@tap", "-h")

    assert result.returncode == 0
    assert "Usage: byk @@@tap" in result.stdout
    for verb in ("list", "add", "remove"):
        assert verb in result.stdout
