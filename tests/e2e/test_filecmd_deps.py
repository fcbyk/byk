import json
import os
import shlex
import tarfile
import time


def write_fake_curl(tmp_path, routes):
    """routes: {URL 后缀: bytes}；`-o` 过来的写文件，其余写 stdout。

    每次调用在日志里留一行 `CALL <url>`，用来断言「到底有没有发生网络请求」。
    """
    bin_dir = tmp_path / "bin"
    bin_dir.mkdir()
    log = tmp_path / "curl-log"

    lines = [
        "#!/bin/sh",
        'out=""; url=""',
        "while [ $# -gt 0 ]; do",
        '  if [ "$1" = "-o" ]; then out="$2"; shift 2; continue; fi',
        '  url="$1"; shift',
        "done",
        f"printf 'CALL %s\\n' \"$url\" >> {shlex.quote(str(log))}",
        'case "$url" in',
    ]
    for index, (suffix, payload) in enumerate(routes.items()):
        payload_path = tmp_path / f"payload-{index}"
        payload_path.write_bytes(payload)
        lines.append(f"  *{suffix}) src={shlex.quote(str(payload_path))} ;;")
    lines += [
        '  *) printf "404\\n" >&2; exit 22 ;;',
        "esac",
        'if [ -n "$out" ]; then cat "$src" > "$out"; else cat "$src"; fi',
    ]

    script = bin_dir / "curl"
    script.write_text("\n".join(lines) + "\n", encoding="utf-8")
    script.chmod(0o755)
    env = {"PATH": f"{bin_dir}{os.pathsep}{os.environ['PATH']}"}
    return env, log


def calls(log):
    if not log.exists():
        return []
    return [
        line[5:]
        for line in log.read_text(encoding="utf-8").splitlines()
        if line.startswith("CALL ")
    ]


def deps_dir(runner, stem="@@tool"):
    return runner.byk_dir / "deps" / stem


def manifest_of(runner, stem="@@tool"):
    path = deps_dir(runner, stem) / ".byk-deps.json"
    if not path.exists():
        return None
    return json.loads(path.read_text(encoding="utf-8"))


def prepare_tool(runner, files, extra=None, base=None):
    deps = {"files": files}
    if base is not None:
        deps["base"] = base
    data = {"$deps": deps}
    if extra:
        data.update(extra)
    runner.prepare_alias_json("tool", data)


def install(runner, env, *args):
    return runner.run("@@tool", "deps", "i", *args, env=env)


def local_base(tmp_path):
    """造一个本机目录当 base，返回落盘形态（正斜杠、结尾带 `/`）。"""
    project = tmp_path / "proj"
    project.mkdir()
    return f"{project}/".replace("\\", "/")


# ── 首次安装 ────────────────────────────────────────────────────────────


def test_fresh_install_downloads_and_records_manifest(runner, tmp_path):
    env, _ = write_fake_curl(tmp_path, {"/bin/tool": b"v1", "/cfg.json": b"cfg"})
    prepare_tool(
        runner,
        [
            {"src": "https://example.com/bin/tool", "to": "bin", "exec": True},
            "https://example.com/cfg.json",
        ],
    )

    result = install(runner, env)

    assert result.returncode == 0
    assert result.stdout == ""
    assert result.stderr.splitlines() == [
        f"Downloading 2 files into {deps_dir(runner)}:",
        "  add     @@tool/bin/tool <- https://example.com/bin/tool",
        "  add     @@tool/cfg.json <- https://example.com/cfg.json",
        f"Downloaded 2 files into {deps_dir(runner)}",
    ]

    assert (deps_dir(runner) / "bin" / "tool").read_bytes() == b"v1"
    if os.name != "nt":
        assert os.access(deps_dir(runner) / "bin" / "tool", os.X_OK)

    manifest = manifest_of(runner)
    assert manifest["version"] == 1
    assert [entry["dest"] for entry in manifest["entries"]] == ["bin/tool", "cfg.json"]
    assert manifest["entries"][0]["exec"] is True


def test_ghproxy_base_routes_absolute_github_urls(runner, tmp_path):
    env, log = write_fake_curl(
        tmp_path,
        {
            "/releases/latest/download/tool.zip": b"zip",
            "/HEAD/tool": b"raw",
        },
    )
    prepare_tool(
        runner,
        [
            "https://github.com/u/r/releases/latest/download/tool.zip",
            "tool",
        ],
        base="https://gh-proxy.com/https://raw.githubusercontent.com/u/r/HEAD/",
    )

    result = install(runner, env)

    assert result.returncode == 0, result.stderr
    # 绝对 GitHub 地址补上代理；相对地址经 base 已带前缀，不能再套一层
    assert calls(log) == [
        "https://gh-proxy.com/https://github.com/u/r/releases/latest/download/tool.zip",
        "https://gh-proxy.com/https://raw.githubusercontent.com/u/r/HEAD/tool",
    ]


def test_second_install_is_a_noop(runner, tmp_path):
    env, log = write_fake_curl(tmp_path, {"/bin/tool": b"v1"})
    prepare_tool(runner, ["https://example.com/bin/tool"])
    install(runner, env)
    before = len(calls(log))

    result = install(runner, env)

    assert result.returncode == 0
    assert "up to date" in result.stderr
    assert len(calls(log)) == before, "no network should happen"


# ── 声明变化 ────────────────────────────────────────────────────────────


def test_changed_url_updates_the_file(runner, tmp_path):
    env, log = write_fake_curl(tmp_path, {"/bin/tool": b"v1", "/v2/tool": b"v2"})
    prepare_tool(runner, ["https://example.com/bin/tool"])
    install(runner, env)

    prepare_tool(runner, ["https://example.com/v2/tool"])
    result = install(runner, env)

    assert result.returncode == 0
    assert result.stderr.splitlines() == [
        f"Syncing 1 declared entry in {deps_dir(runner)}:",
        "  update  @@tool/tool <- https://example.com/v2/tool",
        f"Downloaded 1 file into {deps_dir(runner)}",
    ]
    assert (deps_dir(runner) / "tool").read_bytes() == b"v2"
    assert calls(log)[-1] == "https://example.com/v2/tool"


def test_renamed_entry_adds_and_removes(runner, tmp_path):
    env, _ = write_fake_curl(tmp_path, {"/old-tool": b"old", "/new-tool": b"new"})
    prepare_tool(runner, ["https://example.com/old-tool"])
    install(runner, env)

    prepare_tool(runner, ["https://example.com/new-tool"])
    result = install(runner, env)

    assert result.returncode == 0
    assert result.stderr.splitlines() == [
        f"Syncing 1 declared entry in {deps_dir(runner)}:",
        "  add     @@tool/new-tool <- https://example.com/new-tool",
        "  remove  @@tool/old-tool",
        f"Downloaded 1 file into {deps_dir(runner)}",
        f"Removed 1 file from {deps_dir(runner)}",
    ]
    assert (deps_dir(runner) / "new-tool").is_file()
    assert not (deps_dir(runner) / "old-tool").exists()
    assert [entry["dest"] for entry in manifest_of(runner)["entries"]] == ["new-tool"]


def test_changed_to_removes_the_old_directory(runner, tmp_path):
    env, _ = write_fake_curl(tmp_path, {"/tool": b"x"})
    prepare_tool(runner, [{"src": "https://example.com/tool", "to": "bin"}])
    install(runner, env)

    prepare_tool(runner, [{"src": "https://example.com/tool", "to": "bin2"}])
    result = install(runner, env)

    assert result.returncode == 0
    assert (deps_dir(runner) / "bin2" / "tool").is_file()
    assert not (deps_dir(runner) / "bin").exists()


def test_exec_flag_only_rechmods(runner, tmp_path):
    env, log = write_fake_curl(tmp_path, {"/tool": b"#!/bin/sh\n"})
    prepare_tool(runner, ["https://example.com/tool"])
    install(runner, env)
    before = len(calls(log))

    prepare_tool(runner, [{"src": "https://example.com/tool", "exec": True}])
    result = install(runner, env)

    assert result.returncode == 0
    assert "chmod   @@tool/tool" in result.stderr
    # 只改 exec 不重取：汇总行不该出现
    assert "Downloaded" not in result.stderr
    assert len(calls(log)) == before
    if os.name != "nt":
        assert os.access(deps_dir(runner) / "tool", os.X_OK)


def test_missing_file_is_repaired(runner, tmp_path):
    env, _ = write_fake_curl(tmp_path, {"/bin/tool": b"v1", "/cfg.json": b"cfg"})
    prepare_tool(
        runner,
        ["https://example.com/bin/tool", "https://example.com/cfg.json"],
    )
    install(runner, env)
    (deps_dir(runner) / "cfg.json").unlink()

    result = install(runner, env)

    assert result.returncode == 0
    assert result.stderr.splitlines() == [
        f"Syncing 2 declared entries in {deps_dir(runner)}:",
        "  keep    @@tool/tool",
        "  repair  @@tool/cfg.json <- https://example.com/cfg.json",
        f"Downloaded 1 file into {deps_dir(runner)}",
    ]
    assert (deps_dir(runner) / "cfg.json").read_bytes() == b"cfg"


def test_unknown_file_is_kept_with_a_warning(runner, tmp_path):
    env, _ = write_fake_curl(tmp_path, {"/tool": b"x"})
    prepare_tool(runner, ["https://example.com/tool"])
    install(runner, env)

    stray = deps_dir(runner) / "scratch.txt"
    stray.write_text("mine", encoding="utf-8")

    result = install(runner, env)

    assert result.returncode == 0
    assert "up to date" in result.stderr
    assert "is not declared and was left untouched" in result.stderr
    assert str(stray) in result.stderr
    assert stray.read_text(encoding="utf-8") == "mine"


# ── 清单兼容 ────────────────────────────────────────────────────────────


def test_legacy_directory_is_adopted_without_downloading(runner, tmp_path):
    env, log = write_fake_curl(tmp_path, {"/tool": b"x"})
    prepare_tool(runner, [{"src": "https://example.com/tool", "to": "bin"}])
    legacy = deps_dir(runner) / "bin" / "tool"
    legacy.parent.mkdir(parents=True)
    legacy.write_bytes(b"preinstalled")

    result = install(runner, env)

    assert result.returncode == 0
    assert result.stderr.splitlines() == [
        f"Syncing 1 declared entry in {deps_dir(runner)}:",
        "  adopt   @@tool/bin/tool",
    ]
    assert calls(log) == []
    assert legacy.read_bytes() == b"preinstalled"
    assert manifest_of(runner)["entries"][0]["dest"] == "bin/tool"


def test_broken_manifest_falls_back_to_adopting(runner, tmp_path):
    env, _ = write_fake_curl(tmp_path, {"/tool": b"x"})
    prepare_tool(runner, ["https://example.com/tool"])
    (deps_dir(runner) / "tool").parent.mkdir(parents=True)
    (deps_dir(runner) / "tool").write_bytes(b"preinstalled")
    (deps_dir(runner) / ".byk-deps.json").write_text("{not json", encoding="utf-8")

    result = install(runner, env)

    assert result.returncode == 0
    assert "ignoring .byk-deps.json" in result.stderr
    assert "adopt   @@tool/tool" in result.stderr
    assert manifest_of(runner)["entries"][0]["dest"] == "tool"


def test_manifest_with_escaping_dest_is_rejected(runner):
    prepare_tool(runner, ["https://example.com/tool"])
    deps_dir(runner).mkdir(parents=True)
    (deps_dir(runner) / "tool").write_bytes(b"x")
    (deps_dir(runner) / ".byk-deps.json").write_text(
        json.dumps(
            {
                "version": 1,
                "entries": [
                    {"src": "https://example.com/evil", "dest": "../../victim"}
                ],
            }
        ),
        encoding="utf-8",
    )

    result = runner.run("@@tool", "deps", "i")

    assert result.returncode == 1
    assert 'a ".." segment' in result.stderr
    assert (deps_dir(runner) / "tool").exists()


def test_force_rebuilds_everything(runner, tmp_path):
    env, log = write_fake_curl(tmp_path, {"/tool": b"x"})
    prepare_tool(runner, ["https://example.com/tool"])
    install(runner, env)
    before = len(calls(log))

    result = install(runner, env, "--force")

    assert result.returncode == 0
    assert "--force" in result.stderr
    assert "update  @@tool/tool" in result.stderr
    assert len(calls(log)) == before + 1


def test_partial_failure_keeps_what_landed(runner, tmp_path):
    env, _ = write_fake_curl(tmp_path, {"/bin/tool": b"v1"})
    prepare_tool(
        runner,
        ["https://example.com/bin/tool", "https://example.com/bin/boom"],
    )

    result = install(runner, env)

    assert result.returncode == 1
    assert "already written in this run" in result.stderr
    assert "byk @@tool deps install" in result.stderr
    # 清单只记真正落盘的那条，下一次安装接着做
    assert [entry["dest"] for entry in manifest_of(runner)["entries"]] == ["tool"]


# ── 解压条目 ────────────────────────────────────────────────────────────


def make_tar(tmp_path, name, members):
    path = tmp_path / name
    root = tmp_path / f"{name}-src"
    root.mkdir(exist_ok=True)
    with tarfile.open(path, "w:gz") as tar:
        for member in members:
            item = root / member
            item.write_text(f"{member} content", encoding="utf-8")
            tar.add(item, arcname=member)
    return path.read_bytes()


def test_extract_replaces_the_whole_directory(runner, tmp_path):
    v1 = make_tar(tmp_path, "v1.tar.gz", ["tool", "ghost"])
    v2 = make_tar(tmp_path, "v2.tar.gz", ["tool"])
    env, _ = write_fake_curl(tmp_path, {"/v1.tar.gz": v1, "/v2.tar.gz": v2})
    prepare_tool(
        runner,
        [{"src": "https://example.com/v1.tar.gz", "to": "dist", "extract": True}],
    )
    install(runner, env)
    assert (deps_dir(runner) / "dist" / "ghost").is_file()

    prepare_tool(
        runner,
        [{"src": "https://example.com/v2.tar.gz", "to": "dist", "extract": True}],
    )
    result = install(runner, env)

    assert result.returncode == 0
    assert (
        "update  @@tool/dist <- https://example.com/v2.tar.gz (extract)"
        in result.stderr
    )
    # 整目录替换：旧归档里的 ghost 不能留
    assert (deps_dir(runner) / "dist" / "tool").is_file()
    assert not (deps_dir(runner) / "dist" / "ghost").exists()


def test_root_extract_actually_downloads(runner, tmp_path):
    """解压到专属目录根（没有 to）是最常见的发布包形态，必须真的下载。"""
    archive = make_tar(tmp_path, "v1.tar.gz", ["lansend"])
    env, log = write_fake_curl(tmp_path, {"/v1.tar.gz": archive})
    prepare_tool(
        runner, [{"src": "https://example.com/v1.tar.gz", "extract": True}]
    )

    status = runner.run("@@tool", "deps")
    assert "0 present" in status.stdout
    assert "missing  @@tool" in status.stdout

    result = install(runner, env)

    assert result.returncode == 0
    assert result.stderr.splitlines() == [
        f"Downloading 1 file into {deps_dir(runner)}:",
        "  add     @@tool <- https://example.com/v1.tar.gz (extract)",
        f"Downloaded 1 file into {deps_dir(runner)}",
    ]
    assert (deps_dir(runner) / "lansend").is_file()
    assert len(calls(log)) == 1

    again = install(runner, env)
    assert "up to date" in again.stderr

    ok = runner.run("@@tool", "deps")
    assert "1 present" in ok.stdout
    assert "ok       @@tool" in ok.stdout


def test_root_extract_recorded_over_an_empty_directory_is_repaired(runner, tmp_path):
    """清单说装过、目录却空到只剩清单：补一次，而不是继续报 ok。"""
    archive = make_tar(tmp_path, "v1.tar.gz", ["lansend"])
    env, _ = write_fake_curl(tmp_path, {"/v1.tar.gz": archive})
    prepare_tool(
        runner, [{"src": "https://example.com/v1.tar.gz", "extract": True}]
    )
    root = deps_dir(runner)
    root.mkdir(parents=True)
    (root / ".byk-deps.json").write_text(
        json.dumps(
            {
                "version": 1,
                "entries": [
                    {
                        "src": "https://example.com/v1.tar.gz",
                        "extract": True,
                        "dest": "",
                    }
                ],
            }
        ),
        encoding="utf-8",
    )

    result = install(runner, env)

    assert result.returncode == 0
    assert "repair  @@tool <- https://example.com/v1.tar.gz (extract)" in result.stderr
    assert (root / "lansend").is_file()


def test_root_extract_rejects_other_entries(runner):
    prepare_tool(
        runner,
        [
            {"src": "https://example.com/tool.tar.gz", "extract": True},
            "https://example.com/cfg.json",
        ],
    )

    result = runner.run("@@tool", "deps", "i")

    assert result.returncode == 1
    assert "must be the only entry" in result.stderr


def test_extract_install_is_idempotent(runner, tmp_path):
    archive = make_tar(tmp_path, "v1.tar.gz", ["tool"])
    env, log = write_fake_curl(tmp_path, {"/v1.tar.gz": archive})
    prepare_tool(
        runner,
        [{"src": "https://example.com/v1.tar.gz", "to": "dist", "extract": True}],
    )
    install(runner, env)
    before = len(calls(log))

    result = install(runner, env)

    assert result.returncode == 0
    assert "up to date" in result.stderr
    assert len(calls(log)) == before


# ── 边界 ────────────────────────────────────────────────────────────────


def test_empty_declaration_is_an_error(runner):
    prepare_tool(runner, [])

    result = runner.run("@@tool", "deps", "i")

    assert result.returncode == 1
    assert "nothing to install" in result.stderr


def test_install_rejects_yes_flag(runner):
    prepare_tool(runner, ["https://example.com/tool"])

    result = runner.run("@@tool", "deps", "install", "--yes")

    assert result.returncode == 1
    assert "Unknown option: --yes" in result.stderr
    assert "the only option is --force" in result.stderr


def test_deps_rejects_unknown_subcommand(runner):
    prepare_tool(runner, ["https://example.com/tool"])

    result = runner.run("@@tool", "deps", "upgrade")

    assert result.returncode == 1
    assert "Unknown deps subcommand" in result.stderr
    assert "Available: install, uninstall, status" in result.stderr


# ── 局部依赖：落在项目里 ────────────────────────────────────────────────


def prepare_local(tmp_path, files, name="tool", extra=None):
    data = {"$deps": {"files": files}}
    if extra:
        data.update(extra)
    (tmp_path / f"{name}.byk.json").write_text(json.dumps(data), encoding="utf-8")


def test_local_deps_install_into_the_project(runner, tmp_path):
    env, _ = write_fake_curl(tmp_path, {"/tool": b"local"})
    prepare_local(tmp_path, ["https://example.com/tool"])

    result = runner.run("@tool", "deps", "i", cwd=str(tmp_path), env=env)

    assert result.returncode == 0
    local_deps = tmp_path / ".byk" / "deps" / "@tool"
    assert (local_deps / "tool").read_bytes() == b"local"
    assert (local_deps / ".byk-deps.json").is_file()
    # 自忽略的 .gitignore，免得仓库里冒出二进制
    assert (tmp_path / ".byk" / "deps" / ".gitignore").read_text() == "*\n"
    # 全局那份不受影响
    assert not (runner.byk_dir / "deps" / "@@tool").exists()


def test_local_deps_do_not_mix_between_projects(runner, tmp_path):
    project_a = tmp_path / "a"
    project_b = tmp_path / "b"
    for project, payload in ((project_a, b"A"), (project_b, b"B")):
        project.mkdir()
        env, _ = write_fake_curl(project, {"/tool": payload})
        prepare_local(project, ["https://example.com/tool"])

        result = runner.run("@tool", "deps", "i", cwd=str(project), env=env)
        assert result.returncode == 0, result.stderr

    assert (project_a / ".byk" / "deps" / "@tool" / "tool").read_bytes() == b"A"
    assert (project_b / ".byk" / "deps" / "@tool" / "tool").read_bytes() == b"B"


def test_local_deps_dir_expands_in_the_file(runner, tmp_path):
    prepare_local(
        tmp_path,
        ["https://example.com/tool"],
        extra={"$paths": ["${deps_dir}"], "run": "tool"},
    )

    result = runner.run("@tool", "json", "--expand", cwd=str(tmp_path))

    assert result.returncode == 0
    assert f"{tmp_path}/.byk/deps/@tool" in result.stdout


def test_destroy_removes_local_deps(runner, tmp_path):
    env, _ = write_fake_curl(tmp_path, {"/tool": b"x"})
    prepare_local(tmp_path, ["https://example.com/tool"])
    runner.run("@tool", "deps", "i", cwd=str(tmp_path), env=env)
    assert (tmp_path / ".byk" / "deps" / "@tool").is_dir()

    result = runner.run("@tool", "destroy", "--yes", cwd=str(tmp_path))

    assert result.returncode == 0, result.stderr
    assert not (tmp_path / ".byk").exists(), "空壳目录也要收掉"


def test_check_ignores_install_state_of_local_deps(runner, tmp_path):
    """一条都没装也不该让语法正确的文件变成「有问题」：`--check` 只管配置。"""
    prepare_local(tmp_path, ["https://example.com/tool"])

    result = runner.run("--check", cwd=str(tmp_path))

    assert result.returncode == 0
    assert "no problems" in result.stdout


def test_status_without_declaration_errors(runner):
    runner.prepare_alias_json("tool", {"run": "tool --version"})

    result = runner.run("@@tool", "deps")

    assert result.returncode == 1
    assert 'declares no "$deps"' in result.stderr


# ── status ──────────────────────────────────────────────────────────────


def seed_status_fixture(runner):
    """A 正常、B 缺失、C 过期、D 多出来。"""
    prepare_tool(
        runner,
        [
            {"src": "https://example.com/bin/a", "to": "bin"},
            {"src": "https://example.com/bin/b", "to": "bin"},
            {"src": "https://example.com/cfg/c.json", "to": "cfg"},
        ],
    )
    root = deps_dir(runner)
    (root / "bin").mkdir(parents=True)
    (root / "bin" / "a").write_text("a", encoding="utf-8")
    (root / "cfg").mkdir()
    (root / "cfg" / "c.json").write_text("c", encoding="utf-8")
    (root / "old.bin").write_text("old", encoding="utf-8")
    (root / ".byk-deps.json").write_text(
        json.dumps(
            {
                "version": 1,
                "entries": [
                    {"src": "https://example.com/bin/a", "to": "bin", "dest": "bin/a"},
                    {
                        "src": "https://example.com/v1/c.json",
                        "to": "cfg",
                        "dest": "cfg/c.json",
                    },
                    {"src": "https://example.com/old.bin", "dest": "old.bin"},
                ],
            }
        ),
        encoding="utf-8",
    )


def test_status_reports_four_states(runner):
    seed_status_fixture(runner)

    result = runner.run("@@tool", "deps")

    assert result.returncode == 0
    assert result.stderr == ""
    assert result.stdout.splitlines() == [
        "@@tool: 3 declared, 1 present, 1 stale",
        "  ok       @@tool/bin/a",
        "  missing  @@tool/bin/b",
        "  outdated @@tool/cfg/c.json",
        "  stale    @@tool/old.bin",
        f"Deps directory: {deps_dir(runner)}",
        "Hint: run `byk @@tool deps install` to install the missing assets",
    ]


def test_status_explains_stale_only(runner):
    prepare_tool(runner, [{"src": "https://example.com/bin/a", "to": "bin"}])
    root = deps_dir(runner)
    (root / "bin").mkdir(parents=True)
    (root / "bin" / "a").write_text("a", encoding="utf-8")
    (root / "old.bin").write_text("old", encoding="utf-8")
    (root / ".byk-deps.json").write_text(
        json.dumps(
            {
                "version": 1,
                "entries": [
                    {"src": "https://example.com/bin/a", "to": "bin", "dest": "bin/a"},
                    {"src": "https://example.com/old.bin", "dest": "old.bin"},
                ],
            }
        ),
        encoding="utf-8",
    )

    result = runner.run("@@tool", "deps")

    assert result.returncode == 0
    assert "1 stale" in result.stdout
    assert "to remove the stale assets" in result.stdout


def test_check_ignores_install_state(runner):
    """缺、旧、多余都是运行时状态，`--check` 不报——那是 `deps status` 的事。"""
    seed_status_fixture(runner)

    result = runner.run("--check")

    assert result.returncode == 0
    assert "no problems" in result.stdout
    assert "deps" not in result.stdout


def test_check_is_clean_after_install(runner, tmp_path):
    env, _ = write_fake_curl(tmp_path, {"/tool": b"x"})
    prepare_tool(runner, ["https://example.com/tool"])
    install(runner, env)

    result = runner.run("--check")

    assert result.returncode == 0
    assert "no problems" in result.stdout


def test_deps_completion_accepts_short_prefixes(runner):
    short = runner.run("__api__", "complete", "@@tool", "deps", "u")
    status = runner.run("__api__", "complete", "@@tool", "deps", "s")
    empty = runner.run("__api__", "complete", "@@tool", "deps", "")

    assert short.stdout.splitlines() == ["uninstall"]
    assert status.stdout.splitlines() == ["status"]
    assert empty.stdout.splitlines() == ["install", "uninstall", "status"]


# ── $deps.base ──────────────────────────────────────────────────────────


def test_relative_src_joins_the_base(runner, tmp_path):
    env, log = write_fake_curl(tmp_path, {"/dist/tool": b"v1", "/shared/x": b"x"})
    prepare_tool(
        runner,
        ["tool", "../shared/x"],
        base="https://example.com/dist/",
    )

    result = install(runner, env)

    assert result.returncode == 0, result.stderr
    assert result.stderr.splitlines() == [
        f"Downloading 2 files into {deps_dir(runner)}:",
        "  add     @@tool/tool <- https://example.com/dist/tool",
        "  add     @@tool/x <- https://example.com/shared/x",
        f"Downloaded 2 files into {deps_dir(runner)}",
    ]
    # 清单记的是解析后的地址，相对写法只留在声明里
    assert [entry["src"] for entry in manifest_of(runner)["entries"]] == [
        "https://example.com/dist/tool",
        "https://example.com/shared/x",
    ]


def test_base_is_the_identity_of_the_source(runner, tmp_path):
    env, _ = write_fake_curl(tmp_path, {"/v1/tool": b"v1", "/v2/tool": b"v2"})
    prepare_tool(runner, ["tool"], base="https://example.com/v1/")
    assert install(runner, env).returncode == 0

    # 只改 base，src 一个字没动：来源换了，必须重下
    prepare_tool(runner, ["tool"], base="https://example.com/v2/")
    result = install(runner, env)

    assert result.returncode == 0, result.stderr
    assert f"  update  @@tool/tool <- https://example.com/v2/tool" in result.stderr
    assert (deps_dir(runner) / "tool").read_bytes() == b"v2"


def test_status_shows_the_base(runner, tmp_path):
    env, _ = write_fake_curl(tmp_path, {"/dist/tool": b"v1"})
    prepare_tool(runner, ["tool"], base="https://example.com/dist/")
    assert install(runner, env).returncode == 0

    result = runner.run("@@tool", "deps")

    assert result.returncode == 0
    assert "Base: https://example.com/dist/\n" in result.stdout


def test_status_flags_a_missing_base(runner):
    prepare_tool(runner, ["./tool"])

    result = runner.run("@@tool", "deps")

    assert result.returncode == 1
    assert 'relative path but "$deps.base" is not set' in result.stderr


# ── $deps.base 指向本机目录 ──────────────────────────────────────────────


def test_a_local_source_says_how_many_came_from_disk(runner, tmp_path):
    """汇总行统一说 download，本地那几条在括号里交代。"""
    base = local_base(tmp_path)
    (tmp_path / "proj" / "tool").write_text("v1", encoding="utf-8")
    prepare_tool(runner, ["tool"], base=base)

    result = runner.run("@@tool", "deps", "i")

    deps_root = deps_dir(runner)
    assert result.returncode == 0, result.stderr
    assert result.stderr.splitlines() == [
        f"Downloading 1 file into {deps_root}:",
        f"  add     @@tool/tool <- {base}tool",
        f"Downloaded 1 file into {deps_root} (1 local)",
    ]


def test_a_newer_local_source_is_reinstalled(runner, tmp_path):
    base = local_base(tmp_path)
    (tmp_path / "proj" / "tool").write_text("v1", encoding="utf-8")
    prepare_tool(runner, ["tool"], base=base)
    assert runner.run("@@tool", "deps", "i").returncode == 0

    # URL 可以当内容寻址，本地文件不行：改了源就该重装
    time.sleep(1.1)
    (tmp_path / "proj" / "tool").write_text("v2", encoding="utf-8")
    result = runner.run("@@tool", "deps", "i")

    assert result.returncode == 0, result.stderr
    assert f"  update  @@tool/tool <- {base}tool" in result.stderr
    assert "(1 local)" in result.stderr
    assert (deps_dir(runner) / "tool").read_text(encoding="utf-8") == "v2"


def test_a_local_base_can_mix_with_absolute_urls(runner, tmp_path):
    """一条声明里既有复制的也有下载的：括号只数本地那几条。"""
    base = local_base(tmp_path)
    (tmp_path / "proj" / "tool").write_text("v1", encoding="utf-8")
    env, log = write_fake_curl(tmp_path, {"/remote.js": b"r"})
    prepare_tool(runner, ["tool", "https://example.com/remote.js"], base=base)

    result = install(runner, env)

    deps_root = deps_dir(runner)
    assert result.returncode == 0, result.stderr
    assert result.stderr.splitlines() == [
        f"Downloading 2 files into {deps_root}:",
        f"  add     @@tool/tool <- {base}tool",
        "  add     @@tool/remote.js <- https://example.com/remote.js",
        f"Downloaded 2 files into {deps_root} (1 local)",
    ]
    assert len(calls(log)) == 1
