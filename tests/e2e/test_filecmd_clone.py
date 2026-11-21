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


def test_clone_skips_aliases_marked_clone_false(runner):
    source = runner.prepare_alias_json(
        "src",
        {
            "keep": "echo keep",
            "mine": {"$cmd": "echo mine", "$clone": False},
            "group": {
                "$clone": False,
                "hidden": "echo hidden",
                "shared": {"$clone": True, "run": "echo run"},
            },
        },
    )

    result = runner.run("@@ops..clone", str(source))

    assert result.returncode == 0
    assert "Skipped 2 aliases marked $clone: false" in result.stderr
    assert read(runner.byk_dir / "alias" / "ops.byk.json") == {
        "keep": "echo keep",
        "group": {"shared": {"run": "echo run"}},
    }


def test_clone_refuses_when_every_alias_is_marked(runner):
    source = runner.prepare_alias_json(
        "src", {"$clone": False, "deploy": "echo hi"}
    )

    result = runner.run("@@ops..clone", str(source))

    assert result.returncode == 1
    assert "Nothing to clone" in result.stderr
    assert not (runner.byk_dir / "alias" / "ops.byk.json").exists()


def test_clone_file_level_false_allows_reexposed_children(runner):
    source = runner.prepare_alias_json(
        "src",
        {
            "$clone": False,
            "mine": "echo mine",
            "shared": {"$clone": True, "run": "echo run"},
        },
    )

    result = runner.run("@@ops..clone", str(source))

    assert result.returncode == 0
    assert "Skipped 1 alias marked $clone: false" in result.stderr
    assert read(runner.byk_dir / "alias" / "ops.byk.json") == {
        "shared": {"run": "echo run"}
    }


def test_remote_clone_strips_aliases_marked_clone_false(runner, tmp_path):
    payload = (
        b'{"keep": "echo keep",'
        b' "mine": {"$cmd": "echo mine", "$clone": false}}'
    )
    env, _ = install_fake_curl(tmp_path, payload=payload)

    result = runner.run(
        "@@ops..clone", "https://example.com/tool.byk.json", env=env
    )

    assert result.returncode == 0
    assert read(runner.byk_dir / "alias" / "ops.byk.json") == {
        "keep": "echo keep"
    }


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


def test_clone_bare_path_appends_the_alias_suffix(runner, tmp_path):
    (tmp_path / "src.byk.json").write_text(
        '{"build": "echo hi"}', encoding="utf-8"
    )

    result = runner.run("@@ops..clone", str(tmp_path / "src"))

    assert result.returncode == 0, result.stderr
    assert read(runner.byk_dir / "alias" / "ops.byk.json") == {
        "build": "echo hi"
    }


def test_clone_prefers_the_literal_file_over_its_alias_sibling(runner, tmp_path):
    (tmp_path / "src").write_text('{"build": "echo literal"}', encoding="utf-8")
    (tmp_path / "src.byk.json").write_text(
        '{"build": "echo suffixed"}', encoding="utf-8"
    )

    result = runner.run("@@ops..clone", str(tmp_path / "src"))

    assert result.returncode == 0, result.stderr
    assert read(runner.byk_dir / "alias" / "ops.byk.json") == {
        "build": "echo literal"
    }


def test_clone_can_still_name_the_alias_sibling_explicitly(runner, tmp_path):
    (tmp_path / "src").write_text('{"build": "echo literal"}', encoding="utf-8")
    (tmp_path / "src.byk.json").write_text(
        '{"build": "echo suffixed"}', encoding="utf-8"
    )

    result = runner.run("@@ops..clone", str(tmp_path / "src.byk.json"))

    assert result.returncode == 0, result.stderr
    assert read(runner.byk_dir / "alias" / "ops.byk.json") == {
        "build": "echo suffixed"
    }


def test_clone_falls_back_to_a_literal_path(runner, tmp_path):
    (tmp_path / "src").write_text('{"build": "echo literal"}', encoding="utf-8")

    result = runner.run("@@ops..clone", str(tmp_path / "src"))

    assert result.returncode == 0, result.stderr
    assert read(runner.byk_dir / "alias" / "ops.byk.json") == {
        "build": "echo literal"
    }


def test_clone_trailing_slash_reads_the_anonymous_file(runner, tmp_path):
    project = tmp_path / "proj"
    project.mkdir()
    (project / ".byk.json").write_text(
        '{"build": "echo anonymous"}', encoding="utf-8"
    )

    result = runner.run("@@ops..clone", str(project) + os.sep)

    assert result.returncode == 0, result.stderr
    assert read(runner.byk_dir / "alias" / "ops.byk.json") == {
        "build": "echo anonymous"
    }


def test_clone_directory_name_reads_the_anonymous_file(runner, tmp_path):
    project = tmp_path / "proj"
    project.mkdir()
    (project / ".byk.json").write_text(
        '{"build": "echo anonymous"}', encoding="utf-8"
    )

    result = runner.run("@@ops..clone", str(project))

    assert result.returncode == 0, result.stderr
    assert read(runner.byk_dir / "alias" / "ops.byk.json") == {
        "build": "echo anonymous"
    }


def test_clone_dot_reads_the_anonymous_file(runner, tmp_path):
    (tmp_path / ".byk.json").write_text(
        '{"build": "echo dot"}', encoding="utf-8"
    )

    result = runner.run("@@ops..clone", ".", cwd=str(tmp_path))

    assert result.returncode == 0, result.stderr
    assert read(runner.byk_dir / "alias" / "ops.byk.json") == {
        "build": "echo dot"
    }


def test_clone_keeps_other_extensions_literal(runner, tmp_path):
    (tmp_path / "config.json").write_text(
        '{"build": "echo config"}', encoding="utf-8"
    )

    result = runner.run("@@ops..clone", str(tmp_path / "config.json"))

    assert result.returncode == 0, result.stderr
    assert read(runner.byk_dir / "alias" / "ops.byk.json") == {
        "build": "echo config"
    }


def test_clone_missing_bare_source_lists_candidates(runner, tmp_path):
    result = runner.run("@@ops..clone", str(tmp_path / "missing"))

    assert result.returncode == 1
    assert "Source not found:" in result.stderr
    assert "missing.byk.json" in result.stderr
    assert "Also tried:" in result.stderr


def test_link_uses_the_same_bare_path_resolution(runner, tmp_path):
    source = tmp_path / "src.byk.json"
    source.write_text('{"build": "echo hi"}', encoding="utf-8")
    runner.prepare_alias_file("ops", "{}\n")

    result = runner.run("@@ops..link", str(tmp_path / "src"), cwd=str(tmp_path))

    assert result.returncode == 0, result.stderr
    assert read(runner.byk_dir / "alias" / "ops.byk.json") == [
        str(source.resolve())
    ]


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
    assert "-gh, --github <spec> [<cdn>]" in result.stdout
    assert "Clone from GitHub, optionally through a CDN (raw, jsdelivr, ghproxy)" in result.stdout
    assert "-ge, --gitee <spec>" in result.stdout
    assert "Clone from a Gitee repository" in result.stdout
    assert "-t, --tap <spec>" in result.stdout
    assert "Resolve a key from a local or GitHub byk.tap.json index" in result.stdout


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
    assert "Remote source not found: https://example.com/x.byk.json" in result.stderr
    assert "boom" in result.stderr
    assert "a directory path must end with `/`" in result.stderr
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

    result = runner.run("@@ops..clone", "-gh", "aaa", env=env)

    assert result.returncode == 1
    assert "GitHub source must be" in result.stderr
    assert not args_path.exists()
    assert not (runner.byk_dir / "alias" / "ops.byk.json").exists()


def test_clone_github_bare_repo_reads_root_config(runner, tmp_path):
    payload = b'{"build": "echo root"}'
    env, args_path = install_fake_curl(tmp_path, payload=payload)

    result = runner.run("@@ops..clone", "-gh", "aaa/bb", env=env)

    assert result.returncode == 0
    assert args_path.read_text(encoding="utf-8").splitlines() == [
        "-fsSL",
        "--",
        "https://raw.githubusercontent.com/aaa/bb/HEAD/.byk.json",
    ]
    assert (runner.byk_dir / "alias" / "ops.byk.json").read_bytes() == payload


def test_clone_github_bare_repo_with_ref_reads_root_config(runner, tmp_path):
    env, args_path = install_fake_curl(tmp_path, payload=b"{}")

    result = runner.run("@@ops..clone", "--github", "aaa/bb@v1", env=env)

    assert result.returncode == 0
    assert args_path.read_text(encoding="utf-8").splitlines()[-1] == (
        "https://raw.githubusercontent.com/aaa/bb/v1/.byk.json"
    )


def test_clone_bare_repo_hits_root_for_each_forge(runner, tmp_path):
    payload = b'{"build": "echo root"}'
    cases = [
        (("-gh", "aaa/bb"), "https://raw.githubusercontent.com/aaa/bb/HEAD/.byk.json"),
        (("-ge", "aaa/bb"), "https://gitee.com/aaa/bb/raw/HEAD/.byk.json"),
        (
            ("-gh", "aaa/bb", "jsdelivr"),
            "https://cdn.jsdelivr.net/gh/aaa/bb@HEAD/.byk.json",
        ),
        (
            ("-gh", "aaa/bb", "ghproxy"),
            "https://gh-proxy.com/https://raw.githubusercontent.com/aaa/bb/HEAD/.byk.json",
        ),
    ]
    dest = runner.byk_dir / "alias" / "ops.byk.json"

    for index, (args, url) in enumerate(cases):
        case_dir = tmp_path / str(index)
        case_dir.mkdir()
        env, args_path = install_fake_curl(case_dir, payload=payload)
        result = runner.run("@@ops..clone", *args, env=env)

        assert result.returncode == 0, result.stderr
        assert args_path.read_text(encoding="utf-8").splitlines() == [
            "-fsSL",
            "--",
            url,
        ]
        dest.unlink()


def test_clone_rejects_a_host_in_spec(runner, tmp_path):
    env, args_path = install_fake_curl(tmp_path, payload=b"{}")

    result = runner.run("@@ops..clone", "-gh", "github.com/aaa/bb", env=env)

    assert result.returncode == 1
    assert "must not include a host" in result.stderr
    assert "Did you mean `aaa/bb`?" in result.stderr
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

    result = runner.run("@@ops..clone", "-ge", "aaa", env=env)

    assert result.returncode == 1
    assert "Gitee source must be" in result.stderr
    assert not args_path.exists()


def test_clone_github_through_jsdelivr(runner, tmp_path):
    payload = b'{"build": "echo cdn"}'
    env, args_path = install_fake_curl(tmp_path, payload=payload)

    result = runner.run("@@ops..clone", "-gh", "aaa/bb/sss", "jsdelivr", env=env)

    assert result.returncode == 0
    dest = runner.byk_dir / "alias" / "ops.byk.json"
    assert dest.read_bytes() == payload
    assert args_path.read_text(encoding="utf-8").splitlines() == [
        "-fsSL",
        "--",
        "https://cdn.jsdelivr.net/gh/aaa/bb@HEAD/sss.byk.json",
    ]


def test_clone_github_through_jsdelivr_with_ref(runner, tmp_path):
    payload = b'{"build": "echo cdn"}'
    env, args_path = install_fake_curl(tmp_path, payload=payload)

    result = runner.run(
        "@@ops..clone", "--github", "aaa/bb@v1/", "jsdelivr", "-d", env=env
    )

    assert result.returncode == 0, result.stderr
    assert args_path.read_text(encoding="utf-8").splitlines()[-1] == (
        "https://cdn.jsdelivr.net/gh/aaa/bb@v1/.byk.json"
    )


def test_clone_github_unknown_cdn_skips_curl(runner, tmp_path):
    env, args_path = install_fake_curl(tmp_path, payload=b"{}")

    result = runner.run("@@ops..clone", "-gh", "aaa/bb", "fastgit", env=env)

    assert result.returncode == 1
    assert "Unknown CDN: fastgit" in result.stderr
    assert "raw, jsdelivr, ghproxy" in result.stderr
    assert not args_path.exists()
    assert not (runner.byk_dir / "alias" / "ops.byk.json").exists()


def test_clone_github_with_invalid_spec_skips_curl(runner, tmp_path):
    env, args_path = install_fake_curl(tmp_path, payload=b"{}")

    result = runner.run("@@ops..clone", "-gh", "aaa", "jsdelivr", env=env)

    assert result.returncode == 1
    assert "GitHub source must be" in result.stderr
    assert not args_path.exists()


def curl_urls(log):
    return [line for line in log.read_text(encoding="utf-8").splitlines() if line.startswith("http")]


def write_local_tap(runner, repo, index, files=None):
    """在隔离的 ~/.byk/tap/<repo>/ 下放一份只读索引和可选的目标文件"""
    root = runner.byk_dir / "tap" / repo
    root.mkdir(parents=True, exist_ok=True)
    (root / "byk.tap.json").write_text(json.dumps(index), encoding="utf-8")
    for name, content in (files or {}).items():
        path = root / name
        path.parent.mkdir(parents=True, exist_ok=True)
        if isinstance(content, bytes):
            path.write_bytes(content)
        else:
            path.write_text(content, encoding="utf-8")
    return root


def without_curl(tmp_path):
    """PATH 里没有 curl：本地索引命中时命令仍应成功"""
    bin_dir = tmp_path / "no-curl"
    bin_dir.mkdir(exist_ok=True)
    return {"PATH": str(bin_dir)}


def test_clone_tap_routes_to_a_relative_target(runner, tmp_path):
    env, log = install_routed_curl(
        tmp_path,
        {
            "/byk.tap.json": b'{"build": "aliases/build.byk.json"}',
            "/aliases/build.byk.json": b'{"build": "echo hi"}',
        },
    )

    result = runner.run("@@ops..clone", "--tap", "aaa/bb/build", env=env)

    assert result.returncode == 0, result.stderr
    index = "https://raw.githubusercontent.com/aaa/bb/HEAD/byk.tap.json"
    target = "https://raw.githubusercontent.com/aaa/bb/HEAD/aliases/build.byk.json"
    assert curl_urls(log) == [index, target]
    dest = runner.byk_dir / "alias" / "ops.byk.json"
    assert read(dest) == {"build": "echo hi"}
    assert result.stderr.splitlines() == [
        f"Tap index: remote {index}",
        f"Tap build -> {target}",
        f"Cloned {target} to {dest}",
    ]


def test_clone_tap_short_flag_keeps_the_ref(runner, tmp_path):
    env, log = install_routed_curl(
        tmp_path,
        {
            "/byk.tap.json": b'{"build": "tools/build.byk.json"}',
            "/tools/build.byk.json": b'{"build": "echo ref"}',
        },
    )

    result = runner.run("@@ops..clone", "-t", "aaa/bb@v1/build", env=env)

    assert result.returncode == 0, result.stderr
    assert curl_urls(log) == [
        "https://raw.githubusercontent.com/aaa/bb/v1/byk.tap.json",
        "https://raw.githubusercontent.com/aaa/bb/v1/tools/build.byk.json",
    ]


def test_clone_tap_accepts_a_full_url_target(runner, tmp_path):
    env, log = install_routed_curl(
        tmp_path,
        {
            "/byk.tap.json": b'{"build": "https://example.com/remote/tool.byk.json"}',
            "/remote/tool.byk.json": b'{"run": "echo remote"}',
        },
    )

    result = runner.run("@@ops..clone", "--tap", "aaa/bb/build", env=env)

    assert result.returncode == 0, result.stderr
    assert curl_urls(log) == [
        "https://raw.githubusercontent.com/aaa/bb/HEAD/byk.tap.json",
        "https://example.com/remote/tool.byk.json",
    ]
    assert read(runner.byk_dir / "alias" / "ops.byk.json") == {
        "run": "echo remote"
    }


def test_clone_tap_uses_a_local_index_without_network(runner, tmp_path):
    repo = write_local_tap(
        runner,
        "aaa/bb",
        {"build": "aliases/build.byk.json"},
        {"aliases/build.byk.json": '{"build": "echo local"}'},
    )
    index = (repo / "byk.tap.json").resolve()
    target = (repo / "aliases/build.byk.json").resolve()

    result = runner.run(
        "@@ops..clone", "--tap", "aaa/bb/build", env=without_curl(tmp_path)
    )

    assert result.returncode == 0, result.stderr
    dest = runner.byk_dir / "alias" / "ops.byk.json"
    assert read(dest) == {"build": "echo local"}
    assert result.stderr.splitlines() == [
        f"Tap index: local {index}",
        f"Tap build -> {target}",
        f"Cloned {target} to {dest}",
    ]


def test_clone_tap_local_index_key_miss_never_falls_back(runner, tmp_path):
    write_local_tap(runner, "aaa/bb", {"deploy": "x.byk.json"})
    env, args_path = install_fake_curl(
        tmp_path, payload=b'{"build": "remote.byk.json"}'
    )

    result = runner.run("@@ops..clone", "--tap", "aaa/bb/build", env=env)

    assert result.returncode == 1
    assert "Tap key not found" in result.stderr
    assert "Available: deploy" in result.stderr
    assert "local tap index is authoritative" in result.stderr
    assert not args_path.exists()
    assert not (runner.byk_dir / "alias" / "ops.byk.json").exists()


def test_clone_tap_local_index_parse_error_never_falls_back(runner, tmp_path):
    repo = runner.byk_dir / "tap" / "aaa" / "bb"
    repo.mkdir(parents=True)
    (repo / "byk.tap.json").write_text("{not json", encoding="utf-8")
    env, args_path = install_fake_curl(
        tmp_path, payload=b'{"build": "remote.byk.json"}'
    )

    result = runner.run("@@ops..clone", "--tap", "aaa/bb/build", env=env)

    assert result.returncode == 1
    assert "Failed to parse tap index" in result.stderr
    assert "local tap index is authoritative" in result.stderr
    assert not args_path.exists()


def test_clone_tap_local_index_that_is_a_directory_is_an_error(runner, tmp_path):
    path = runner.byk_dir / "tap" / "aaa" / "bb" / "byk.tap.json"
    path.mkdir(parents=True)
    env, args_path = install_fake_curl(tmp_path, payload=b"{}")

    result = runner.run("@@ops..clone", "--tap", "aaa/bb/build", env=env)

    assert result.returncode == 1
    assert "Tap index is not a file" in result.stderr
    assert not args_path.exists()


def test_clone_tap_local_index_has_no_path_sugar(runner, tmp_path):
    write_local_tap(
        runner,
        "aaa/bb",
        {"build": "build"},
        {"build.byk.json": '{"build": "echo sugar"}'},
    )

    result = runner.run(
        "@@ops..clone", "--tap", "aaa/bb/build", env=without_curl(tmp_path)
    )

    assert result.returncode == 1
    assert "Source not found" in result.stderr
    assert "local tap index is authoritative" in result.stderr
    assert not (runner.byk_dir / "alias" / "ops.byk.json").exists()


def test_clone_tap_local_index_treats_at_names_as_literal_paths(runner, tmp_path):
    write_local_tap(
        runner,
        "aaa/bb",
        {"build": "@shared.byk.json"},
        {"@shared.byk.json": '{"build": "echo at"}'},
    )

    result = runner.run(
        "@@ops..clone", "--tap", "aaa/bb/build", env=without_curl(tmp_path)
    )

    assert result.returncode == 0, result.stderr
    assert read(runner.byk_dir / "alias" / "ops.byk.json") == {"build": "echo at"}


def test_clone_tap_local_index_accepts_an_absolute_target(runner, tmp_path):
    absolute = tmp_path / "abs.byk.json"
    absolute.write_text('{"build": "echo abs"}', encoding="utf-8")
    write_local_tap(runner, "aaa/bb", {"build": str(absolute)})

    result = runner.run(
        "@@ops..clone", "--tap", "aaa/bb/build", env=without_curl(tmp_path)
    )

    assert result.returncode == 0, result.stderr
    assert f"Tap build -> {absolute.resolve()}" in result.stderr
    assert read(runner.byk_dir / "alias" / "ops.byk.json") == {"build": "echo abs"}


def test_clone_tap_local_index_can_route_to_a_url(runner, tmp_path):
    repo = write_local_tap(
        runner,
        "aaa/bb",
        {"build": "https://example.com/remote/tool.byk.json"},
    )
    env, log = install_routed_curl(
        tmp_path, {"/remote/tool.byk.json": b'{"run": "echo remote"}'}
    )

    result = runner.run("@@ops..clone", "--tap", "aaa/bb/build", env=env)

    assert result.returncode == 0, result.stderr
    assert f"Tap index: local {(repo / 'byk.tap.json').resolve()}" in result.stderr
    assert "Tap build -> https://example.com/remote/tool.byk.json" in result.stderr
    # 本地索引不联网，只有目标 URL 被下载
    assert curl_urls(log) == ["https://example.com/remote/tool.byk.json"]
    assert read(runner.byk_dir / "alias" / "ops.byk.json") == {
        "run": "echo remote"
    }


def test_clone_tap_ref_skips_the_local_index(runner, tmp_path):
    write_local_tap(
        runner,
        "aaa/bb",
        {"build": "local.byk.json"},
        {"local.byk.json": '{"build": "echo local"}'},
    )
    env, log = install_routed_curl(
        tmp_path,
        {
            "/byk.tap.json": b'{"build": "remote.byk.json"}',
            "/remote.byk.json": b'{"build": "echo remote"}',
        },
    )

    result = runner.run("@@ops..clone", "--tap", "aaa/bb@v1/build", env=env)

    assert result.returncode == 0, result.stderr
    assert (
        "Tap index: remote "
        "https://raw.githubusercontent.com/aaa/bb/v1/byk.tap.json"
        in result.stderr
    )
    assert curl_urls(log) == [
        "https://raw.githubusercontent.com/aaa/bb/v1/byk.tap.json",
        "https://raw.githubusercontent.com/aaa/bb/v1/remote.byk.json",
    ]
    assert read(runner.byk_dir / "alias" / "ops.byk.json") == {
        "build": "echo remote"
    }


def test_clone_tap_local_index_anchors_the_base_on_the_local_target(runner, tmp_path):
    repo = write_local_tap(
        runner,
        "aaa/bb",
        {"build": "pkg/tool.byk.json"},
        {
            "pkg/tool.byk.json": (
                '{"$deps": {"files": ["./bin/tool"]}, "run": "tool"}'
            ),
            "pkg/bin/tool": "tool",
        },
    )

    result = runner.run(
        "@@ops..clone", "--tap", "aaa/bb/build", "-d", env=without_curl(tmp_path)
    )

    assert result.returncode == 0, result.stderr
    base = f"{(repo / 'pkg').resolve()}/".replace("\\", "/")
    assert f"Recorded $deps.base = {base}" in result.stderr
    doc = read(runner.byk_dir / "alias" / "ops.byk.json")
    assert doc["$deps"]["base"] == base
    installed = runner.byk_dir / "deps" / "@@ops" / "tool"
    assert installed.read_text(encoding="utf-8") == "tool"


def test_clone_tap_missing_local_index_falls_back_to_remote(runner, tmp_path):
    env, log = install_routed_curl(
        tmp_path,
        {
            "/byk.tap.json": b'{"build": "aliases/build.byk.json"}',
            "/aliases/build.byk.json": b'{"build": "echo remote"}',
        },
    )

    result = runner.run("@@ops..clone", "--tap", "aaa/bb/build", env=env)

    assert result.returncode == 0, result.stderr
    assert (
        "Tap index: remote "
        "https://raw.githubusercontent.com/aaa/bb/HEAD/byk.tap.json"
        in result.stderr
    )
    assert not (runner.byk_dir / "tap").exists()
    assert read(runner.byk_dir / "alias" / "ops.byk.json") == {
        "build": "echo remote"
    }


def test_clone_tap_single_key_uses_a_unique_local_match(runner, tmp_path):
    repo = write_local_tap(
        runner,
        "aaa/bb",
        {"build": "aliases/build.byk.json"},
        {"aliases/build.byk.json": '{"build": "echo single"}'},
    )
    write_local_tap(runner, "ccc/dd", {"deploy": "deploy.byk.json"})

    result = runner.run(
        "@@ops..clone", "--tap", "build", env=without_curl(tmp_path)
    )

    assert result.returncode == 0, result.stderr
    index = (repo / "byk.tap.json").resolve()
    target = (repo / "aliases/build.byk.json").resolve()
    dest = runner.byk_dir / "alias" / "ops.byk.json"
    assert read(dest) == {"build": "echo single"}
    assert result.stderr.splitlines() == [
        f"Tap index: local {index}",
        f"Tap build -> {target}",
        f"Cloned {target} to {dest}",
    ]


def test_clone_tap_single_key_not_found_never_uses_the_network(runner, tmp_path):
    write_local_tap(runner, "aaa/bb", {"deploy": "deploy.byk.json"})
    env, args_path = install_fake_curl(
        tmp_path, payload=b'{"build": "remote.byk.json"}'
    )

    result = runner.run("@@ops..clone", "--tap", "build", env=env)

    assert result.returncode == 1
    assert 'Tap key "build" was not found in 1 local tap index' in result.stderr
    assert "use the full form" in result.stderr
    assert not args_path.exists()
    assert not (runner.byk_dir / "alias" / "ops.byk.json").exists()


def test_clone_tap_single_key_lists_full_forms_when_ambiguous(runner, tmp_path):
    write_local_tap(
        runner, "ccc/dd", {"build": "c.byk.json"}, {"c.byk.json": "{}"}
    )
    write_local_tap(
        runner, "aaa/bb", {"build": "a.byk.json"}, {"a.byk.json": "{}"}
    )
    env, args_path = install_fake_curl(tmp_path, payload=b"{}")

    result = runner.run("@@ops..clone", "--tap", "build", env=env)

    assert result.returncode == 1
    assert result.stderr.splitlines() == [
        'Tap key "build" exists in multiple local repositories. Use the full form:',
        "  aaa/bb/build",
        "  ccc/dd/build",
    ]
    assert not args_path.exists()
    assert not (runner.byk_dir / "alias" / "ops.byk.json").exists()


def test_clone_tap_single_key_warns_and_skips_a_broken_index(runner, tmp_path):
    broken = runner.byk_dir / "tap" / "aaa" / "bb"
    broken.mkdir(parents=True)
    (broken / "byk.tap.json").write_text("{not json", encoding="utf-8")
    repo = write_local_tap(
        runner,
        "ccc/dd",
        {"build": "build.byk.json"},
        {"build.byk.json": '{"build": "echo good"}'},
    )

    result = runner.run(
        "@@ops..clone", "--tap", "build", env=without_curl(tmp_path)
    )

    assert result.returncode == 0, result.stderr
    lines = result.stderr.splitlines()
    assert lines[0].startswith("Warning: skipped local tap index aaa/bb:")
    assert lines[1] == f"Tap index: local {(repo / 'byk.tap.json').resolve()}"
    assert read(runner.byk_dir / "alias" / "ops.byk.json") == {
        "build": "echo good"
    }


def test_clone_tap_single_key_accepts_a_url_target(runner, tmp_path):
    write_local_tap(
        runner,
        "aaa/bb",
        {"build": "https://example.com/remote/tool.byk.json"},
    )
    env, log = install_routed_curl(
        tmp_path, {"/remote/tool.byk.json": b'{"run": "echo remote"}'}
    )

    result = runner.run("@@ops..clone", "--tap", "build", env=env)

    assert result.returncode == 0, result.stderr
    assert "Tap build -> https://example.com/remote/tool.byk.json" in result.stderr
    assert curl_urls(log) == ["https://example.com/remote/tool.byk.json"]
    assert read(runner.byk_dir / "alias" / "ops.byk.json") == {
        "run": "echo remote"
    }


def test_clone_tap_single_key_missing_target_errors_without_network(runner, tmp_path):
    write_local_tap(runner, "aaa/bb", {"build": "missing.byk.json"})

    result = runner.run(
        "@@ops..clone", "--tap", "build", env=without_curl(tmp_path)
    )

    assert result.returncode == 1
    assert "Tap index: local" in result.stderr
    assert "Source not found" in result.stderr
    assert not (runner.byk_dir / "alias" / "ops.byk.json").exists()


def test_clone_tap_single_key_reports_a_missing_tap_root(runner, tmp_path):
    env, args_path = install_fake_curl(tmp_path, payload=b"{}")

    result = runner.run("@@ops..clone", "--tap", "build", env=env)

    assert result.returncode == 1
    assert "No local tap repositories found at" in result.stderr
    assert not args_path.exists()


def test_clone_tap_missing_index_reports_where_to_add_it(runner, tmp_path):
    env, _ = install_fake_curl(tmp_path, exit_code=22, stderr="404: Not Found")

    result = runner.run("@@ops..clone", "--tap", "aaa/bb/build", env=env)

    assert result.returncode == 1
    assert (
        "Tap index not found: "
        "https://raw.githubusercontent.com/aaa/bb/HEAD/byk.tap.json"
        in result.stderr
    )
    assert "add byk.tap.json at the repository root" in result.stderr
    assert not (runner.byk_dir / "alias" / "ops.byk.json").exists()


def test_clone_tap_missing_key_lists_available_keys(runner, tmp_path):
    payload = b'{"deploy": "x", "build": "y", "$schema": "z"}'
    env, args_path = install_fake_curl(tmp_path, payload=payload)

    result = runner.run("@@ops..clone", "--tap", "aaa/bb/nope", env=env)

    assert result.returncode == 1
    assert "Tap key not found" in result.stderr
    assert "Available: build, deploy" in result.stderr
    assert "$schema" not in result.stderr
    # 只取了索引，没有为不存在的键发第二次请求
    assert args_path.read_text(encoding="utf-8").count("http") == 1


def test_clone_tap_rejects_a_non_string_entry(runner, tmp_path):
    env, _ = install_fake_curl(tmp_path, payload=b'{"build": 42}')

    result = runner.run("@@ops..clone", "--tap", "aaa/bb/build", env=env)

    assert result.returncode == 1
    assert 'Tap entry "build" must be a string' in result.stderr
    assert not (runner.byk_dir / "alias" / "ops.byk.json").exists()


def test_clone_tap_refuses_a_target_that_is_another_index(runner, tmp_path):
    env, log = install_routed_curl(
        tmp_path, {"/byk.tap.json": b'{"build": "byk.tap.json"}'}
    )

    result = runner.run("@@ops..clone", "--tap", "aaa/bb/build", env=env)

    assert result.returncode == 1
    assert "not another tap index" in result.stderr
    # 只路由一跳：第二次请求不该发出
    assert len(curl_urls(log)) == 1
    assert not (runner.byk_dir / "alias" / "ops.byk.json").exists()


def test_clone_tap_is_mutually_exclusive_with_forge_sources(runner, tmp_path):
    env, args_path = install_fake_curl(tmp_path, payload=b"{}")

    result = runner.run(
        "@@ops..clone", "--tap", "aaa/bb/build", "-gh", "aaa/bb", env=env
    )

    assert result.returncode == 1
    assert "clone takes exactly one source" in result.stderr
    assert not args_path.exists()


def test_clone_tap_invalid_spec_skips_curl(runner, tmp_path):
    env, args_path = install_fake_curl(tmp_path, payload=b"{}")

    result = runner.run("@@ops..clone", "--tap", "aaa/bb", env=env)

    assert result.returncode == 1
    assert "GitHub tap source must be" in result.stderr
    assert not args_path.exists()


def test_clone_tap_anchors_the_base_on_the_target_file(runner, tmp_path):
    env, log = install_routed_curl(
        tmp_path,
        {
            "/byk.tap.json": b'{"build": "pkg/deep/tool.byk.json"}',
            "/pkg/deep/tool.byk.json": (
                b'{"$deps": {"files": ["./bin/tool"]}, "run": "tool"}'
            ),
            "/pkg/deep/bin/tool": b"tool",
        },
    )

    result = runner.run("@@ops..clone", "--tap", "aaa/bb/build", "-d", env=env)

    assert result.returncode == 0, result.stderr
    base = "https://raw.githubusercontent.com/aaa/bb/HEAD/pkg/deep/"
    assert f"Recorded $deps.base = {base}" in result.stderr
    doc = read(runner.byk_dir / "alias" / "ops.byk.json")
    assert doc["$deps"]["base"] == base
    # 落点由 `to` 决定（这里缺省，落在依赖目录根），与 src 的目录结构无关
    assert (runner.byk_dir / "deps" / "@@ops" / "tool").read_bytes() == b"tool"
    assert curl_urls(log) == [
        "https://raw.githubusercontent.com/aaa/bb/HEAD/byk.tap.json",
        "https://raw.githubusercontent.com/aaa/bb/HEAD/pkg/deep/tool.byk.json",
        base + "bin/tool",
    ]


def test_clone_forge_options_are_completed(runner):
    short = runner.run("__api__", "complete", "@@ops..clone", "-g")
    long = runner.run("__api__", "complete", "@@ops..clone", "--")
    tap = runner.run("__api__", "complete", "@@ops..clone", "-t")
    cdn = runner.run("__api__", "complete", "@@ops..clone", "-gh", "aaa/bb", "")
    both = runner.run(
        "__api__", "complete", "--with-descriptions", "@@ops..clone", "-"
    )

    assert short.stdout.splitlines() == ["-gh", "-ge"]
    assert long.stdout.splitlines() == [
        "--github",
        "--gitee",
        "--tap",
        "--deps",
    ]
    assert tap.stdout.splitlines() == ["-t"]
    assert cdn.stdout.splitlines() == ["raw", "jsdelivr", "ghproxy"]
    assert both.stdout.splitlines() == [
        "-gh\tClone from GitHub, optionally through a CDN (raw, jsdelivr, ghproxy)",
        "--github\tClone from GitHub, optionally through a CDN (raw, jsdelivr, ghproxy)",
        "-ge\tClone from a Gitee repository",
        "--gitee\tClone from a Gitee repository",
        "-t\tResolve a key from a local or GitHub byk.tap.json index",
        "--tap\tResolve a key from a local or GitHub byk.tap.json index",
        "-d\tAlso install the cloned file's dependencies",
        "--deps\tAlso install the cloned file's dependencies",
    ]


def install_routed_curl(tmp_path, routes):
    """clone 走 stdout、deps 走 `-o`，同一个假 curl 两种都要伺候"""
    bin_dir = tmp_path / "bin"
    bin_dir.mkdir()
    log = tmp_path / "curl-log"

    lines = [
        "#!/bin/sh",
        f"printf '%s\\n' \"$@\" >> {shlex.quote(str(log))}",
        'out=""; url=""',
        "while [ $# -gt 0 ]; do",
        '  if [ "$1" = "-o" ]; then out="$2"; shift 2; continue; fi',
        '  url="$1"; shift',
        "done",
        'case "$url" in',
    ]
    for index, (suffix, payload) in enumerate(routes.items()):
        payload_path = tmp_path / f"payload-{index}"
        payload_path.write_bytes(payload)
        lines.append(f"  *{suffix}) src={shlex.quote(str(payload_path))} ;;")
    lines += [
        "esac",
        'if [ -n "$out" ]; then cat "$src" > "$out"; else cat "$src"; fi',
    ]

    script = bin_dir / "curl"
    script.write_text("\n".join(lines) + "\n", encoding="utf-8")
    script.chmod(0o755)
    env = {"PATH": f"{bin_dir}{os.pathsep}{os.environ['PATH']}"}
    return env, log


WITH_DEPS = (
    b'{"$deps": {"files": [{"src": "https://example.com/bin/tool", "to": "bin"}]},'
    b' "run": "tool"}'
)

WITH_TWO_DEPS = (
    b'{"$deps": {"files": [{"src": "https://example.com/bin/tool", "to": "bin"},'
    b' {"src": "https://example.com/bin/missing", "to": "bin"}]}, "run": "tool"}'
)


def test_clone_install_deps_skips_without_declaration(runner):
    source = runner.prepare_alias_json("src", {"build": "echo hi"})

    result = runner.run("@@ops..clone", str(source), "-d")

    assert result.returncode == 0
    assert "Cloned" in result.stderr
    assert "declares no $deps" in result.stderr
    assert not (runner.byk_dir / "deps" / "@@ops").exists()


def test_clone_install_deps_combines_with_forge(runner, tmp_path):
    env, log = install_routed_curl(tmp_path, {"/.byk.json": b'{"build": "echo hi"}'})

    result = runner.run("@@ops..clone", "-gh", "aaa/bb@v1/", "-d", env=env)

    assert result.returncode == 0
    assert "declares no $deps" in result.stderr
    assert "raw.githubusercontent.com/aaa/bb/v1/.byk.json" in log.read_text(
        encoding="utf-8"
    )


def test_clone_into_local_installs_into_the_project(runner, tmp_path):
    env, _ = install_routed_curl(
        tmp_path,
        {"/hello.byk.json": WITH_DEPS, "/bin/tool": b"local"},
    )

    result = runner.run(
        "@ops..clone",
        "https://example.com/hello.byk.json",
        "-d",
        cwd=str(tmp_path),
        env=env,
    )

    assert result.returncode == 0, result.stderr
    local_deps = tmp_path / ".byk" / "deps" / "@ops"
    assert (local_deps / "bin" / "tool").read_bytes() == b"local"
    assert (local_deps / ".byk-deps.json").is_file()
    assert not (runner.byk_dir / "deps" / "@@ops").exists()


def test_clone_install_deps_installs_without_prompt(runner, tmp_path):
    payload = b"#!/bin/sh\necho hi\n"
    env, _ = install_routed_curl(
        tmp_path,
        {"/hello.byk.json": WITH_DEPS, "/bin/tool": payload},
    )

    # 没有 tty、没有 stdin：安装不再二次确认
    result = runner.run(
        "@@ops..clone", "https://example.com/hello.byk.json", "-d", env=env
    )

    assert result.returncode == 0
    deps_root = runner.byk_dir / "deps" / "@@ops"
    assert result.stderr.splitlines() == [
        f"Cloned https://example.com/hello.byk.json to {runner.byk_dir / 'alias' / 'ops.byk.json'}",
        "Recorded $deps.base = https://example.com/",
        f"Downloading 1 file into {deps_root}:",
        "  add     @@ops/bin/tool <- https://example.com/bin/tool",
        f"Downloaded 1 file into {deps_root}",
    ]
    assert (deps_root / "bin" / "tool").read_bytes() == payload


def test_clone_install_deps_reports_failure(runner, tmp_path):
    # 第二条资产 404、第一条已经落盘：克隆保留，错误里给重试命令
    env, _ = install_routed_curl(
        tmp_path,
        {"/hello.byk.json": WITH_TWO_DEPS, "/bin/tool": b"x"},
    )

    result = runner.run(
        "@@ops..clone", "https://example.com/hello.byk.json", "-d", env=env
    )

    assert result.returncode == 1
    assert "Failed to install dependencies" in result.stderr
    assert "already written in this run" in result.stderr
    assert "byk @@ops..deps i" in result.stderr
    assert (runner.byk_dir / "alias" / "ops.byk.json").is_file()


def test_clone_rejects_yes_without_install_deps(runner):
    source = runner.prepare_alias_json("src", {"build": "echo hi"})

    result = runner.run("@@ops..clone", str(source), "-y")

    assert result.returncode == 1
    assert "Unknown option: -y" in result.stderr
    assert not (runner.byk_dir / "alias" / "ops.byk.json").exists()


def test_resolve_rejects_clone(runner):
    result = runner.run("__api__", "resolve", "@@ops..clone")

    assert result.returncode == 1
    assert "file command" in result.stderr


WITH_RELATIVE_DEPS = (
    b'{"$deps": {"files": ["../hhh.js", "bin/tool"]}, "run": "tool"}'
)


def test_remote_clone_freezes_the_base_and_resolves_relative_src(runner, tmp_path):
    env, _ = install_routed_curl(
        tmp_path,
        {
            "/aaa/bbb/hello.byk.json": WITH_RELATIVE_DEPS,
            "/aaa/hhh.js": b"hhh",
            "/aaa/bbb/bin/tool": b"tool",
        },
    )

    result = runner.run(
        "@@ops..clone", "https://example.com/aaa/bbb/hello.byk.json", "-d", env=env
    )

    assert result.returncode == 0, result.stderr
    assert "Recorded $deps.base = https://example.com/aaa/bbb/" in result.stderr

    doc = read(runner.byk_dir / "alias" / "ops.byk.json")
    assert doc["$deps"]["base"] == "https://example.com/aaa/bbb/"
    # 声明保持原样：相对写法留在文件里，解析结果只出现在清单里
    assert doc["$deps"]["files"] == ["../hhh.js", "bin/tool"]

    deps_root = runner.byk_dir / "deps" / "@@ops"
    assert (deps_root / "hhh.js").read_bytes() == b"hhh"
    # 落点由 `to` 决定，与 src 的目录结构无关
    assert (deps_root / "tool").read_bytes() == b"tool"


def test_remote_clone_resolves_a_relative_base(runner, tmp_path):
    body = b'{"$deps": {"base": "..", "files": ["shared/x.js"]}}'
    env, _ = install_routed_curl(
        tmp_path,
        {"/a/b/hello.byk.json": body, "/a/shared/x.js": b"x"},
    )

    result = runner.run(
        "@@ops..clone", "https://example.com/a/b/hello.byk.json", "-d", env=env
    )

    assert result.returncode == 0, result.stderr
    doc = read(runner.byk_dir / "alias" / "ops.byk.json")
    assert doc["$deps"]["base"] == "https://example.com/a/"
    assert (runner.byk_dir / "deps" / "@@ops" / "x.js").read_bytes() == b"x"


def test_local_clone_freezes_the_base_to_the_source_directory(runner, tmp_path):
    project = tmp_path / "proj"
    project.mkdir()
    source = project / "src.byk.json"
    source.write_text(
        '{"$deps": {"base": ".", "files": ["./tool"]}}', encoding="utf-8"
    )
    (project / "tool").write_text("#!/bin/sh\necho tool\n", encoding="utf-8")

    cloned = runner.run("@@ops..clone", str(source))
    assert cloned.returncode == 0, cloned.stderr
    want = f"{project}/".replace("\\", "/")
    assert f"Recorded $deps.base = {want}" in cloned.stderr

    doc = read(runner.byk_dir / "alias" / "ops.byk.json")
    assert doc["$deps"]["base"] == want
    # 声明一字未改：相对写法留在文件里，解析结果只出现在清单里
    assert doc["$deps"]["files"] == ["./tool"]

    result = runner.run("@@ops..deps", "i")
    assert result.returncode == 0, result.stderr
    installed = runner.byk_dir / "deps" / "@@ops" / "tool"
    assert installed.read_text(encoding="utf-8") == "#!/bin/sh\necho tool\n"


def test_local_clone_installs_without_any_network(runner, tmp_path):
    project = tmp_path / "proj"
    project.mkdir()
    (project / "tool").write_bytes(b"binary")
    source = project / "src.byk.json"
    source.write_text('{"$deps": {"files": ["./tool"]}}', encoding="utf-8")

    # PATH 里没有 curl：本地来源根本不该调用它
    result = runner.run(
        "@@ops..clone",
        str(source),
        "-d",
        env={"PATH": str(tmp_path / "empty-bin")},
    )

    assert result.returncode == 0, result.stderr
    assert (runner.byk_dir / "deps" / "@@ops" / "tool").read_bytes() == b"binary"


def test_local_deps_i_reports_a_missing_source(runner, tmp_path):
    project = tmp_path / "proj"
    project.mkdir()
    source = project / "src.byk.json"
    source.write_text('{"$deps": {"files": ["./tool"]}}', encoding="utf-8")

    assert runner.run("@@ops..clone", str(source)).returncode == 0
    # 源文件后来没了：静默跳过会让人以为装好了
    result = runner.run("@@ops..deps", "i")

    assert result.returncode == 1
    assert "cannot read" in result.stderr
    assert "tool" in result.stderr


def test_deps_i_reports_a_relative_src_without_base(runner):
    runner.prepare_alias_json(
        "ops", {"$deps": {"files": ["../hhh.js"]}, "run": "tool"}
    )

    result = runner.run("@@ops..deps", "i")

    assert result.returncode == 1
    assert 'relative path but "$deps.base" is not set' in result.stderr


def test_remote_clone_rejects_a_variable_base(runner, tmp_path):
    # clone 期没有变量表：${CDN} 求不出值，与其写死一个假地址不如直接失败
    body = json.dumps(
        {
            "$var": {"CDN": "https://example.com/cdn/"},
            "$deps": {"base": "${CDN}", "files": ["x.js"]},
        }
    ).encode()
    env, _ = install_routed_curl(tmp_path, {"/p/var.byk.json": body})

    result = runner.run(
        "@@ops..clone", "https://example.com/p/var.byk.json", "-d", env=env
    )

    assert result.returncode == 1
    assert "must not contain variables" in result.stderr
    # 半坏文件比没有更糟：拒绝时不落盘
    assert not (runner.byk_dir / "alias" / "ops.byk.json").exists()


def test_remote_clone_rejects_a_leading_slash_base(runner, tmp_path):
    # 前导 `/` 跟本地绝对路径分不开，不再当包根
    body = b'{"$deps": {"base": "/sub", "files": ["x.js"]}}'
    env, _ = install_routed_curl(tmp_path, {"/p/slash.byk.json": body})

    result = runner.run(
        "@@ops..clone", "https://example.com/p/slash.byk.json", env=env
    )

    assert result.returncode == 1
    assert "is a local path" in result.stderr


def test_deps_i_names_an_unexpanded_variable_in_base(runner):
    runner.prepare_alias_json(
        "ops", {"$deps": {"base": "${NOPE}", "files": ["x.js"]}}
    )

    result = runner.run("@@ops..deps", "i")

    assert result.returncode == 1
    assert '"${NOPE}" was not expanded' in result.stderr
