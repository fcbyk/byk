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
        ("-gh", "https://raw.githubusercontent.com/aaa/bb/HEAD/.byk.json"),
        ("-ge", "https://gitee.com/aaa/bb/raw/HEAD/.byk.json"),
        ("-jd", "https://cdn.jsdelivr.net/gh/aaa/bb@HEAD/.byk.json"),
    ]
    dest = runner.byk_dir / "alias" / "ops.byk.json"

    for flag, url in cases:
        case_dir = tmp_path / flag.strip("-")
        case_dir.mkdir()
        env, args_path = install_fake_curl(case_dir, payload=payload)
        result = runner.run("@@ops..clone", flag, "aaa/bb", env=env)

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

    result = runner.run("@@ops..clone", "-jd", "aaa", env=env)

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
    assert long.stdout.splitlines() == [
        "--github",
        "--gitee",
        "--jsdelivr",
        "--deps",
    ]
    assert jd.stdout.splitlines() == ["-jd"]
    assert both.stdout.splitlines() == [
        "-gh\tClone from a GitHub repository",
        "--github\tClone from a GitHub repository",
        "-ge\tClone from a Gitee repository",
        "--gitee\tClone from a Gitee repository",
        "-jd\tClone a GitHub file through jsDelivr",
        "--jsdelivr\tClone a GitHub file through jsDelivr",
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
