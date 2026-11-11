import os


def test_rename_global_named(runner):
    old = runner.prepare_alias_json("ops", {"a": "echo a"})

    result = runner.run("@@ops..rename", "new")

    new = runner.byk_dir / "alias" / "new.byk.json"
    assert result.returncode == 0
    assert result.stdout == ""
    assert "Renamed" in result.stderr
    assert new.read_text() == '{"a": "echo a"}'
    assert not old.exists()


def test_rename_local_uses_cwd(runner, tmp_path):
    old = tmp_path / "ops.byk.json"
    old.write_text('{"a": "echo a"}', encoding="utf-8")

    result = runner.run("@ops..rename", "new", cwd=str(tmp_path))

    assert result.returncode == 0
    assert (tmp_path / "new.byk.json").read_text() == '{"a": "echo a"}'
    assert not old.exists()


def test_rename_to_anonymous(runner, tmp_path):
    runner.prepare_alias_json("ops", {})

    global_result = runner.run("@@ops..rename", "")
    (tmp_path / "local.byk.json").write_text("{}", encoding="utf-8")
    local_result = runner.run("@local..rename", "", cwd=str(tmp_path))

    assert global_result.returncode == 0
    assert (runner.byk_dir / "alias" / ".byk.json").exists()
    assert local_result.returncode == 0
    assert (tmp_path / ".byk.json").exists()


def test_rename_refuses_existing_target(runner):
    old = runner.prepare_alias_json("ops", {"a": "echo a"})
    existing = runner.prepare_alias_json("new", {"b": "echo b"})

    result = runner.run("@@ops..rename", "new")

    assert result.returncode == 1
    assert "Target already exists" in result.stderr
    assert old.read_text() == '{"a": "echo a"}'
    assert existing.read_text() == '{"b": "echo b"}'


def test_rename_refuses_same_name(runner):
    path = runner.prepare_alias_json("ops", {})

    result = runner.run("@@ops..rename", "ops")

    assert result.returncode == 1
    assert "Source and target are the same" in result.stderr
    assert path.exists()


def test_rename_missing_source(runner):
    result = runner.run("@@ops..rename", "new")

    assert result.returncode == 1
    assert "File not found" in result.stderr


def test_rename_rejects_wrong_argument_count(runner):
    runner.prepare_alias_json("ops", {})

    missing = runner.run("@@ops..rename")
    extra = runner.run("@@ops..rename", "a", "b")

    assert missing.returncode == 1
    assert "exactly one argument" in missing.stderr
    assert extra.returncode == 1
    assert "exactly one argument" in extra.stderr


def test_rename_rejects_invalid_name(runner):
    path = runner.prepare_alias_json("ops", {})

    result = runner.run("@@ops..rename", "a.b")

    assert result.returncode == 1
    assert "Invalid file name" in result.stderr
    assert path.exists()


def test_rename_symlink_renames_link_only(runner, tmp_path):
    real = tmp_path / "real.byk.json"
    real.write_text("{}", encoding="utf-8")
    alias_dir = runner.byk_dir / "alias"
    alias_dir.mkdir(parents=True)
    link = alias_dir / "ops.byk.json"
    os.symlink(real, link)

    result = runner.run("@@ops..rename", "new")

    new = alias_dir / "new.byk.json"
    assert result.returncode == 0
    assert new.is_symlink()
    assert os.readlink(new) == str(real)
    assert real.exists()
    assert not link.exists()


def test_rename_refuses_dangling_symlink_target(runner, tmp_path):
    source = runner.prepare_alias_json("ops", {})
    target = runner.byk_dir / "alias" / "new.byk.json"
    os.symlink(tmp_path / "gone.byk.json", target)

    result = runner.run("@@ops..rename", "new")

    assert result.returncode == 1
    assert "Target already exists" in result.stderr
    assert target.is_symlink()
    assert source.exists()


def test_rename_is_completed_with_description(runner):
    result = runner.run("__api__", "complete", "--with-descriptions", "@@ops..r")

    assert result.returncode == 0
    assert result.stdout.splitlines() == ["@@ops..rename\tRename the alias file"]


def test_resolve_rejects_rename(runner):
    result = runner.run("__api__", "resolve", "@@ops..rename")

    assert result.returncode == 1
    assert "file command" in result.stderr
