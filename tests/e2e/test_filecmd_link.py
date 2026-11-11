import json
import os


def write_alias(path, data):
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(json.dumps(data), encoding="utf-8")
    return path


def link_target(path):
    return os.path.realpath(json.loads(path.read_text())[0])


def test_init_link_creates_global_link_usable_anywhere(runner, tmp_path):
    repo = tmp_path / "repo"
    elsewhere = tmp_path / "elsewhere"
    elsewhere.mkdir()
    source = write_alias(
        repo / "tool.byk.json",
        {
            "hello": "echo hi",
            "where": 'test -f "${file_dir}/tool.byk.json" && echo yes',
        },
    )

    result = runner.run("@@tools..init", "--link", "@tool", cwd=str(repo))

    index = runner.byk_dir / "alias" / "tools.byk.json"
    assert result.returncode == 0, result.stderr
    assert result.stdout == ""
    assert "Created" in result.stderr
    assert link_target(index) == os.path.realpath(source)
    assert index.read_text().endswith("\n")
    assert runner.run("hello", cwd=str(elsewhere)).stdout.strip() == "hi"
    assert runner.run("where", cwd=str(elsewhere)).stdout.strip() == "yes"
    assert runner.run("@@tools.hello", cwd=str(elsewhere)).stdout.strip() == "hi"


def test_init_link_works_in_local_scope(runner, tmp_path):
    source = write_alias(tmp_path / "tool.byk.json", {"hello": "echo hi"})

    result = runner.run("@tools..init", "--link", "@tool", cwd=str(tmp_path))

    index = tmp_path / "tools.byk.json"
    assert result.returncode == 0, result.stderr
    assert link_target(index) == os.path.realpath(source)
    assert runner.run("@tools.hello", cwd=str(tmp_path)).stdout.strip() == "hi"
    assert runner.run("hello", cwd=str(tmp_path)).stdout.strip() == "hi"


def test_init_link_accepts_anonymous_alias_target(runner, tmp_path):
    source = write_alias(tmp_path / ".byk.json", {"hello": "echo hi"})

    result = runner.run("@@tools..init", "--link", "@", cwd=str(tmp_path))

    assert result.returncode == 0, result.stderr
    assert link_target(runner.byk_dir / "alias" / "tools.byk.json") == os.path.realpath(
        source
    )


def test_init_link_accepts_paths(runner, tmp_path):
    repo = tmp_path / "repo"
    elsewhere = tmp_path / "elsewhere"
    elsewhere.mkdir()
    relative = write_alias(repo / "rel.byk.json", {"rel": "echo rel"})
    absolute = write_alias(tmp_path / "abs.byk.json", {"abs": "echo abs"})
    home = write_alias(runner.home / "home.byk.json", {"home": "echo home"})

    relative_result = runner.run(
        "@@relative..init", "--link", "./rel.byk.json", cwd=str(repo)
    )
    absolute_result = runner.run(
        "@@absolute..init", "--link", str(absolute), cwd=str(repo)
    )
    home_result = runner.run(
        "@@home..init", "--link", "~/home.byk.json", cwd=str(repo)
    )

    alias_dir = runner.byk_dir / "alias"
    assert relative_result.returncode == 0, relative_result.stderr
    assert absolute_result.returncode == 0, absolute_result.stderr
    assert home_result.returncode == 0, home_result.stderr
    assert link_target(alias_dir / "relative.byk.json") == os.path.realpath(relative)
    assert link_target(alias_dir / "absolute.byk.json") == os.path.realpath(absolute)
    assert link_target(alias_dir / "home.byk.json") == os.path.realpath(home)


def test_init_link_refuses_missing_target(runner, tmp_path):
    result = runner.run("@@tools..init", "--link", "@missing", cwd=str(tmp_path))

    assert result.returncode == 1
    assert "Link target not found" in result.stderr
    assert not (runner.byk_dir / "alias" / "tools.byk.json").exists()


def test_init_link_refuses_non_object_target(runner, tmp_path):
    target = tmp_path / "scalar.byk.json"
    target.write_text("42", encoding="utf-8")

    result = runner.run("@@tools..init", "--link", "@scalar", cwd=str(tmp_path))

    assert result.returncode == 1
    assert "must be a JSON object" in result.stderr
    assert not (runner.byk_dir / "alias" / "tools.byk.json").exists()


def test_init_link_refuses_link_target(runner, tmp_path):
    write_alias(tmp_path / "other.byk.json", {"a": "echo a"})
    (tmp_path / "chain.byk.json").write_text('["other.byk.json"]\n', encoding="utf-8")

    result = runner.run("@@tools..init", "--link", "@chain", cwd=str(tmp_path))

    assert result.returncode == 1
    assert "is itself a link" in result.stderr
    assert not (runner.byk_dir / "alias" / "tools.byk.json").exists()


def test_init_link_refuses_invalid_json_target(runner, tmp_path):
    (tmp_path / "broken.byk.json").write_text("{not json", encoding="utf-8")

    result = runner.run("@@tools..init", "--link", "@broken", cwd=str(tmp_path))

    assert result.returncode == 1
    assert "Failed to parse link target" in result.stderr


def test_init_link_rejects_invalid_alias_target(runner, tmp_path):
    result = runner.run("@@tools..init", "--link", "@a.b", cwd=str(tmp_path))

    assert result.returncode == 1
    assert "Invalid file name" in result.stderr


def test_init_link_refuses_existing_file(runner, tmp_path):
    write_alias(tmp_path / "tool.byk.json", {"hello": "echo hi"})
    existing = runner.prepare_alias_file("tools", '{"keep": true}')

    result = runner.run("@@tools..init", "--link", "@tool", cwd=str(tmp_path))

    assert result.returncode == 1
    assert "already exists" in result.stderr
    assert existing.read_text() == '{"keep": true}'


def test_link_converts_empty_object(runner, tmp_path):
    source = write_alias(tmp_path / "tool.byk.json", {"hello": "echo hi"})
    runner.run("@@tools..init")

    result = runner.run("@@tools..link", "@tool", cwd=str(tmp_path))

    index = runner.byk_dir / "alias" / "tools.byk.json"
    assert result.returncode == 0, result.stderr
    assert "Linked" in result.stderr
    assert link_target(index) == os.path.realpath(source)


def test_link_retargets_existing_link(runner, tmp_path):
    first = write_alias(tmp_path / "first.byk.json", {"a": "echo a"})
    second = write_alias(tmp_path / "second.byk.json", {"b": "echo b"})
    runner.run("@@tools..init", "--link", "@first", cwd=str(tmp_path))
    index = runner.byk_dir / "alias" / "tools.byk.json"

    result = runner.run("@@tools..link", "@second", cwd=str(tmp_path))

    assert result.returncode == 0, result.stderr
    assert link_target(index) == os.path.realpath(second)
    assert first.exists()


def test_link_refuses_non_empty_object(runner, tmp_path):
    write_alias(tmp_path / "tool.byk.json", {"hello": "echo hi"})
    existing = runner.prepare_alias_file("tools", '{"keep": "echo keep"}')

    result = runner.run("@@tools..link", "@tool", cwd=str(tmp_path))

    assert result.returncode == 1
    assert "Not a link" in result.stderr
    assert existing.read_text() == '{"keep": "echo keep"}'


def test_link_missing_file_hints_init(runner, tmp_path):
    write_alias(tmp_path / "tool.byk.json", {"hello": "echo hi"})

    result = runner.run("@@tools..link", "@tool", cwd=str(tmp_path))

    assert result.returncode == 1
    assert "File not found" in result.stderr
    assert "@@tools..init --link" in result.stderr


def test_link_refuses_self_link(runner, tmp_path):
    runner.run("@tools..init", cwd=str(tmp_path))
    path = tmp_path / "tools.byk.json"

    result = runner.run("@tools..link", "@tools", cwd=str(tmp_path))

    assert result.returncode == 1
    assert "Link cannot point to itself" in result.stderr
    assert path.read_text() == "{}\n"


def test_link_refuses_symlink(runner, tmp_path):
    real = tmp_path / "real.byk.json"
    real.write_text("{}\n", encoding="utf-8")
    alias_dir = runner.byk_dir / "alias"
    alias_dir.mkdir(parents=True)
    link = alias_dir / "tools.byk.json"
    os.symlink(real, link)
    write_alias(tmp_path / "tool.byk.json", {"hello": "echo hi"})

    result = runner.run("@@tools..link", "@tool", cwd=str(tmp_path))

    assert result.returncode == 1
    assert "Refusing to rewrite symlink" in result.stderr
    assert real.read_text() == "{}\n"
    assert link.is_symlink()


def test_link_requires_exactly_one_target(runner):
    missing = runner.run("@@tools..link")
    extra = runner.run("@@tools..link", "@a", "@b")

    assert missing.returncode == 1
    assert "exactly one target" in missing.stderr
    assert extra.returncode == 1
    assert "exactly one target" in extra.stderr


def test_init_link_option_is_completed_with_description(runner):
    result = runner.run("__api__", "complete", "--with-descriptions", "@@ops..init", "--")

    assert result.returncode == 0
    assert result.stdout.splitlines() == [
        "--link\tCreate as a link to another alias file"
    ]


def test_link_is_completed_with_description(runner):
    result = runner.run("__api__", "complete", "--with-descriptions", "@@ops..l")

    assert result.returncode == 0
    assert result.stdout.splitlines() == [
        "@@ops..link\tPoint this file at another alias file"
    ]


def test_resolve_rejects_link(runner):
    result = runner.run("__api__", "resolve", "@@tools..link")

    assert result.returncode == 1
    assert "file command" in result.stderr
