import json
import os
import shlex


def read(path):
    return json.loads(path.read_text(encoding="utf-8"))


def install_fake_curl(tmp_path, payload=None, exit_code=0, stderr=""):
    bin_dir = tmp_path / "bin"
    bin_dir.mkdir()
    args_path = tmp_path / "curl-args"
    lines = [
        "#!/bin/sh",
        f"printf '%s\\n' \"$@\" > {shlex.quote(str(args_path))}",
    ]
    if payload is not None:
        payload_path = tmp_path / "payload"
        payload_path.write_bytes(payload)
        lines.append(f"cat {shlex.quote(str(payload_path))}")
    if stderr:
        lines.append(f"printf '%s\\n' {shlex.quote(stderr)} >&2")
    if exit_code:
        lines.append(f"exit {exit_code}")
    script = bin_dir / "curl"
    script.write_text("\n".join(lines) + "\n", encoding="utf-8")
    script.chmod(0o755)
    env = {"PATH": f"{bin_dir}{os.pathsep}{os.environ['PATH']}"}
    return env, args_path


def test_clone_copies_source_bytes(runner):
    source = runner.prepare_alias_file(
        "src", '{\n  "z": "echo z",\n\n  "a": "echo a"\n}'
    )

    result = runner.run("@@ops..clone", str(source))

    assert result.returncode == 0
    assert result.stdout == ""
    assert "Cloned" in result.stderr
    dest = runner.byk_dir / "alias" / "ops.byk.json"
    assert dest.read_text(encoding="utf-8") == (
        '{\n  "z": "echo z",\n\n  "a": "echo a"\n}'
    )


def test_clone_global_name(runner):
    runner.prepare_alias_json("src", {"build": "echo hi"})

    result = runner.run("@@ops..clone", "@@src")

    assert result.returncode == 0
    assert read(runner.byk_dir / "alias" / "ops.byk.json") == {
        "build": "echo hi"
    }


def test_clone_local_name(runner, tmp_path):
    source = tmp_path / "src.byk.json"
    source.write_text('{"build": "echo local"}', encoding="utf-8")

    result = runner.run("@@ops..clone", "@src", cwd=str(tmp_path))

    assert result.returncode == 0
    assert read(runner.byk_dir / "alias" / "ops.byk.json") == {
        "build": "echo local"
    }


def test_clone_to_local_destination(runner, tmp_path):
    source = runner.prepare_alias_json("src", {"build": "echo hi"})

    result = runner.run("@ops..clone", str(source), cwd=str(tmp_path))

    assert result.returncode == 0
    assert read(tmp_path / "ops.byk.json") == {"build": "echo hi"}


def test_clone_refuses_existing_destination(runner):
    source = runner.prepare_alias_json("src", {"build": "echo hi"})
    dest = runner.prepare_alias_json("ops", {"keep": "echo keep"})

    result = runner.run("@@ops..clone", str(source))

    assert result.returncode == 1
    assert "File already exists" in result.stderr
    assert read(dest) == {"keep": "echo keep"}


def test_clone_refuses_link_source(runner):
    source = runner.prepare_alias_file("src", '["shared.byk.json"]')
    target = runner.prepare_alias_json("shared", {"build": "echo hi"})

    result = runner.run("@@ops..clone", str(source))

    assert result.returncode == 1
    assert "Source is itself a link" in result.stderr
    assert not (runner.byk_dir / "alias" / "ops.byk.json").exists()
    assert read(target) == {"build": "echo hi"}


def test_clone_missing_source(runner):
    missing = runner.home / "missing.byk.json"

    result = runner.run("@@ops..clone", str(missing))

    assert result.returncode == 1
    assert "Source not found" in result.stderr


def test_clone_refuses_directory_source(runner):
    source = runner.home / "dir.byk.json"
    source.mkdir()

    result = runner.run("@@ops..clone", str(source))

    assert result.returncode == 1
    assert "Source is not a file" in result.stderr


def test_clone_invalid_json_source(runner):
    source = runner.prepare_alias_file("src", "{not json")

    result = runner.run("@@ops..clone", str(source))

    assert result.returncode == 1
    assert "Failed to parse source" in result.stderr


def test_clone_non_object_source(runner):
    source = runner.prepare_alias_file("src", "42")

    result = runner.run("@@ops..clone", str(source))

    assert result.returncode == 1
    assert "Source must be a JSON object" in result.stderr


def test_clone_rejects_invalid_name(runner):
    result = runner.run("@@ops..clone", "@@a/b")

    assert result.returncode == 1
    assert "Invalid file name" in result.stderr


def test_clone_rejects_extra_arguments(runner):
    source = runner.prepare_alias_json("src", {"build": "echo hi"})

    result = runner.run("@@ops..clone", str(source), "extra")

    assert result.returncode == 1
    assert "clone takes exactly one source" in result.stderr
    assert not (runner.byk_dir / "alias" / "ops.byk.json").exists()


def test_clone_source_completion_lists_alias_files(runner, tmp_path):
    runner.prepare_alias_json("src", {"build": "echo hi"})
    (tmp_path / "local.byk.json").write_text("{}", encoding="utf-8")

    global_only = runner.run("__api__", "complete", "@@ops..clone", "@@")
    both = runner.run("__api__", "complete", "@@ops..clone", "@", cwd=str(tmp_path))
    path_partial = runner.run("__api__", "complete", "@@ops..clone", "./")

    assert global_only.returncode == 0
    assert global_only.stdout.splitlines() == ["@@src"]
    assert both.returncode == 0
    assert both.stdout.splitlines() == ["@@src", "@local"]
    assert path_partial.returncode == 0
    assert path_partial.stdout == ""


def test_link_and_init_link_complete_sources(runner):
    runner.prepare_alias_json("src", {"build": "echo hi"})

    link = runner.run("__api__", "complete", "@@ops..link", "@@")
    init = runner.run("__api__", "complete", "@@ops..init", "--link", "@@")

    assert link.returncode == 0
    assert link.stdout.splitlines() == ["@@src"]
    assert init.returncode == 0
    assert init.stdout.splitlines() == ["@@src"]


def test_help_for_clone_shows_usage(runner):
    result = runner.run("@@ops..help", "clone")

    assert result.returncode == 0
    assert result.stdout.startswith("\n@@ops..clone <source> [options]\n")
    assert "Copy a local or remote alias file into this file" in result.stdout
    assert "-gh, --github <spec>" in result.stdout
    assert "Clone from a GitHub repository" in result.stdout
    assert "-ge, --gitee <spec>" in result.stdout
    assert "Clone from a Gitee repository" in result.stdout
    assert "-jd, --jsdelivr <spec>" in result.stdout
    assert "Clone a GitHub file through jsDelivr" in result.stdout


def test_clone_url_uses_curl(runner, tmp_path):
    url = "https://example.com/tool.byk.json"
    payload = b'{\n  "z": "echo z",\n\n  "a": "echo a"\n}'
    env, args_path = install_fake_curl(tmp_path, payload=payload)

    result = runner.run("@@ops..clone", url, env=env)

    assert result.returncode == 0
    assert url in result.stderr
    dest = runner.byk_dir / "alias" / "ops.byk.json"
    assert dest.read_bytes() == payload
    assert args_path.read_text(encoding="utf-8").splitlines() == [
        "-fsSL",
        "--",
        url,
    ]


def test_clone_url_rejects_invalid_json(runner, tmp_path):
    env, _ = install_fake_curl(tmp_path, payload=b"{not json")

    result = runner.run("@@ops..clone", "https://example.com/x.byk.json", env=env)

    assert result.returncode == 1
    assert "Failed to parse source https://example.com/x.byk.json" in result.stderr
    assert not (runner.byk_dir / "alias" / "ops.byk.json").exists()


def test_clone_url_rejects_non_object(runner, tmp_path):
    env, _ = install_fake_curl(tmp_path, payload=b"42")

    result = runner.run("@@ops..clone", "https://example.com/x.byk.json", env=env)

    assert result.returncode == 1
    assert "Source must be a JSON object" in result.stderr
    assert not (runner.byk_dir / "alias" / "ops.byk.json").exists()


def test_clone_url_reports_curl_failure(runner, tmp_path):
    env, _ = install_fake_curl(tmp_path, exit_code=22, stderr="boom")

    result = runner.run("@@ops..clone", "https://example.com/x.byk.json", env=env)

    assert result.returncode == 1
    assert "curl failed (exit 22)" in result.stderr
    assert "boom" in result.stderr
    assert not (runner.byk_dir / "alias" / "ops.byk.json").exists()


def test_clone_url_requires_curl(runner, tmp_path):
    empty_bin = tmp_path / "empty"
    empty_bin.mkdir()

    result = runner.run(
        "@@ops..clone",
        "https://example.com/x.byk.json",
        env={"PATH": str(empty_bin)},
    )

    assert result.returncode == 1
    assert "Remote clone requires curl" in result.stderr
    assert "install curl" in result.stderr
    assert not (runner.byk_dir / "alias" / "ops.byk.json").exists()


def test_clone_rejects_unsupported_scheme(runner):
    result = runner.run("@@ops..clone", "ftp://example.com/x.byk.json")

    assert result.returncode == 1
    assert "Unsupported source scheme" in result.stderr
    assert "only http and https" in result.stderr


def test_clone_existing_destination_skips_curl(runner, tmp_path):
    payload = b'{"build": "echo hi"}'
    env, args_path = install_fake_curl(tmp_path, payload=payload)
    dest = runner.prepare_alias_json("ops", {"keep": "echo keep"})

    result = runner.run("@@ops..clone", "https://example.com/x.byk.json", env=env)

    assert result.returncode == 1
    assert "File already exists" in result.stderr
    assert not args_path.exists()
    assert read(dest) == {"keep": "echo keep"}


def test_link_rejects_url(runner):
    runner.prepare_alias_file("ops", "{}\n")

    result = runner.run("@@ops..link", "https://example.com/x.byk.json")

    assert result.returncode == 1
    assert "Remote source is not supported here" in result.stderr
    assert "use ..clone" in result.stderr


def test_clone_github_shorthand(runner, tmp_path):
    payload = b'{"build": "echo hi"}'
    env, args_path = install_fake_curl(tmp_path, payload=payload)

    result = runner.run("@@ops..clone", "-gh", "aaa/bb/sss", env=env)

    assert result.returncode == 0
    dest = runner.byk_dir / "alias" / "ops.byk.json"
    assert dest.read_bytes() == payload
    assert args_path.read_text(encoding="utf-8").splitlines() == [
        "-fsSL",
        "--",
        "https://raw.githubusercontent.com/aaa/bb/HEAD/sss.byk.json",
    ]


def test_clone_github_long_flag_with_ref(runner, tmp_path):
    payload = b'{"build": "echo hi"}'
    env, args_path = install_fake_curl(tmp_path, payload=payload)

    result = runner.run("@@ops..clone", "--github", "aaa/bb@v1/", env=env)

    assert result.returncode == 0
    assert args_path.read_text(encoding="utf-8").splitlines()[-1] == (
        "https://raw.githubusercontent.com/aaa/bb/v1/.byk.json"
    )


def test_clone_github_requires_curl(runner, tmp_path):
    empty_bin = tmp_path / "empty"
    empty_bin.mkdir()

    result = runner.run(
        "@@ops..clone", "-gh", "aaa/bb/sss", env={"PATH": str(empty_bin)}
    )

    assert result.returncode == 1
    assert "Remote clone requires curl" in result.stderr
    assert not (runner.byk_dir / "alias" / "ops.byk.json").exists()


def test_clone_github_invalid_spec_skips_curl(runner, tmp_path):
    env, args_path = install_fake_curl(tmp_path, payload=b"{}")

    result = runner.run("@@ops..clone", "-gh", "aaa/bb", env=env)

    assert result.returncode == 1
    assert "GitHub source must be" in result.stderr
    assert not args_path.exists()
    assert not (runner.byk_dir / "alias" / "ops.byk.json").exists()


def test_clone_gitee_shorthand(runner, tmp_path):
    payload = b'{"build": "echo gitee"}'
    env, args_path = install_fake_curl(tmp_path, payload=payload)

    result = runner.run("@@ops..clone", "-ge", "aaa/bb/sss", env=env)

    assert result.returncode == 0
    dest = runner.byk_dir / "alias" / "ops.byk.json"
    assert dest.read_bytes() == payload
    assert args_path.read_text(encoding="utf-8").splitlines() == [
        "-fsSL",
        "--",
        "https://gitee.com/aaa/bb/raw/HEAD/sss.byk.json",
    ]


def test_clone_gitee_long_flag_with_ref(runner, tmp_path):
    payload = b'{"build": "echo gitee"}'
    env, args_path = install_fake_curl(tmp_path, payload=payload)

    result = runner.run("@@ops..clone", "--gitee", "aaa/bb@v1/", env=env)

    assert result.returncode == 0
    assert args_path.read_text(encoding="utf-8").splitlines()[-1] == (
        "https://gitee.com/aaa/bb/raw/v1/.byk.json"
    )


def test_clone_gitee_invalid_spec_skips_curl(runner, tmp_path):
    env, args_path = install_fake_curl(tmp_path, payload=b"{}")

    result = runner.run("@@ops..clone", "-ge", "aaa/bb", env=env)

    assert result.returncode == 1
    assert "Gitee source must be" in result.stderr
    assert not args_path.exists()


def test_clone_jsdelivr_shorthand(runner, tmp_path):
    payload = b'{"build": "echo cdn"}'
    env, args_path = install_fake_curl(tmp_path, payload=payload)

    result = runner.run("@@ops..clone", "-jd", "aaa/bb/sss", env=env)

    assert result.returncode == 0
    dest = runner.byk_dir / "alias" / "ops.byk.json"
    assert dest.read_bytes() == payload
    assert args_path.read_text(encoding="utf-8").splitlines() == [
        "-fsSL",
        "--",
        "https://cdn.jsdelivr.net/gh/aaa/bb@HEAD/sss.byk.json",
    ]


def test_clone_jsdelivr_long_flag_with_ref(runner, tmp_path):
    payload = b'{"build": "echo cdn"}'
    env, args_path = install_fake_curl(tmp_path, payload=payload)

    result = runner.run("@@ops..clone", "--jsdelivr", "aaa/bb@v1/", env=env)

    assert result.returncode == 0
    assert args_path.read_text(encoding="utf-8").splitlines()[-1] == (
        "https://cdn.jsdelivr.net/gh/aaa/bb@v1/.byk.json"
    )


def test_clone_jsdelivr_invalid_spec_skips_curl(runner, tmp_path):
    env, args_path = install_fake_curl(tmp_path, payload=b"{}")

    result = runner.run("@@ops..clone", "-jd", "aaa/bb", env=env)

    assert result.returncode == 1
    assert "jsDelivr source must be" in result.stderr
    assert not args_path.exists()


def test_clone_forge_options_are_completed(runner):
    short = runner.run("__api__", "complete", "@@ops..clone", "-g")
    long = runner.run("__api__", "complete", "@@ops..clone", "--")
    jd = runner.run("__api__", "complete", "@@ops..clone", "-j")
    both = runner.run(
        "__api__", "complete", "--with-descriptions", "@@ops..clone", "-"
    )

    assert short.stdout.splitlines() == ["-gh", "-ge"]
    assert long.stdout.splitlines() == ["--github", "--gitee", "--jsdelivr"]
    assert jd.stdout.splitlines() == ["-jd"]
    assert both.stdout.splitlines() == [
        "-gh\tClone from a GitHub repository",
        "--github\tClone from a GitHub repository",
        "-ge\tClone from a Gitee repository",
        "--gitee\tClone from a Gitee repository",
        "-jd\tClone a GitHub file through jsDelivr",
        "--jsdelivr\tClone a GitHub file through jsDelivr",
    ]


def test_resolve_rejects_clone(runner):
    result = runner.run("__api__", "resolve", "@@ops..clone")

    assert result.returncode == 1
    assert "file command" in result.stderr
