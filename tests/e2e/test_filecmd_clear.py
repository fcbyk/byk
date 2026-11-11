import os


def test_clear_object_with_yes(runner):
    path = runner.prepare_alias_json("ops", {"build": "echo build"})

    result = runner.run("@@ops..clear", "--yes")

    assert result.returncode == 0
    assert result.stdout == ""
    assert "Cleared" in result.stderr
    assert path.read_text() == "{}\n"


def test_clear_link_file_does_not_touch_target(runner):
    target = runner.prepare_alias_json("shared", {"build": "echo hi"})
    link_file = runner.prepare_alias_file("ops", '["shared.byk.json"]')

    result = runner.run("@@ops..clear", "--yes")

    assert result.returncode == 0
    assert link_file.read_text() == "{}\n"
    assert target.read_text() == '{"build": "echo hi"}'


def test_clear_local_and_anonymous(runner, tmp_path):
    local = tmp_path / "ops.byk.json"
    local.write_text('{"build": "echo local"}', encoding="utf-8")
    anonymous = runner.prepare_alias_file("", '{"build": "echo anon"}')

    local_result = runner.run("@ops..clear", "--yes", cwd=str(tmp_path))
    anonymous_result = runner.run("@@..clear", "--yes")

    assert local_result.returncode == 0
    assert local.read_text() == "{}\n"
    assert anonymous_result.returncode == 0
    assert anonymous.read_text() == "{}\n"


def test_clear_already_empty_is_a_noop(runner):
    path = runner.prepare_alias_file("ops", "{}\n")

    result = runner.run("@@ops..clear")

    assert result.returncode == 0
    assert "Already empty" in result.stderr
    assert path.read_text() == "{}\n"


def test_clear_non_tty_requires_yes(runner):
    path = runner.prepare_alias_json("ops", {"build": "echo build"})

    result = runner.run("@@ops..clear")

    assert result.returncode == 1
    assert "--yes" in result.stderr
    assert path.read_text() == '{"build": "echo build"}'


def test_clear_interactive_requires_matching_stem(runner):
    path = runner.prepare_alias_json(
        "ops", {"build": "echo build", "group": {"test": "echo test"}}
    )

    result = runner.run("@@ops..clear", input="ops\n", tty_stderr=True)

    assert result.returncode == 0
    assert "This will clear:" in result.stderr
    assert "(2 aliases)" in result.stderr
    assert "Type" in result.stderr
    assert "ops:" in result.stderr
    assert path.read_text() == "{}\n"


def test_clear_interactive_wrong_input_cancels(runner):
    path = runner.prepare_alias_json("ops", {"build": "echo build"})

    result = runner.run("@@ops..clear", input="nope\n", tty_stderr=True)

    assert result.returncode == 0
    assert "Confirmation does not match. Cancelled." in result.stderr
    assert path.read_text() == '{"build": "echo build"}'


def test_clear_interactive_eof_cancels(runner):
    path = runner.prepare_alias_json("ops", {"build": "echo build"})

    result = runner.run("@@ops..clear", input="", tty_stderr=True)

    assert result.returncode == 0
    assert "Cancelled." in result.stderr
    assert path.read_text() == '{"build": "echo build"}'


def test_clear_interactive_link_note(runner):
    target = runner.prepare_alias_json("shared", {"build": "echo hi"})
    link_file = runner.prepare_alias_file("ops", '["shared.byk.json"]')

    result = runner.run("@@ops..clear", input="ops\n", tty_stderr=True)

    assert result.returncode == 0
    assert "link file; its target will not be touched" in result.stderr
    assert link_file.read_text() == "{}\n"
    assert target.read_text() == '{"build": "echo hi"}'


def test_clear_interactive_unreadable_note(runner):
    path = runner.prepare_alias_file("ops", "{not json")

    result = runner.run("@@ops..clear", input="ops\n", tty_stderr=True)

    assert result.returncode == 0
    assert "(content cannot be summarized)" in result.stderr
    assert path.read_text() == "{}\n"


def test_clear_invalid_json_with_yes(runner):
    path = runner.prepare_alias_file("ops", "{not json")

    result = runner.run("@@ops..clear", "--yes")

    assert result.returncode == 0
    assert path.read_text() == "{}\n"


def test_clear_refuses_symlink(runner):
    real = runner.prepare_alias_json("real", {"build": "echo hi"})
    link_file = runner.byk_dir / "alias" / "ops.byk.json"
    os.symlink(real, link_file)

    result = runner.run("@@ops..clear", "--yes")

    assert result.returncode == 1
    assert "Refusing to clear symlink" in result.stderr
    assert real.read_text() == '{"build": "echo hi"}'
    assert link_file.is_symlink()


def test_clear_refuses_directory(runner):
    path = runner.byk_dir / "alias" / "ops.byk.json"
    path.mkdir(parents=True)

    result = runner.run("@@ops..clear", "--yes")

    assert result.returncode == 1
    assert "Refusing to clear directory" in result.stderr
    assert path.is_dir()


def test_clear_missing_file(runner):
    result = runner.run("@@ops..clear", "--yes")

    assert result.returncode == 1
    assert "File not found" in result.stderr


def test_clear_rejects_extra_arguments(runner):
    path = runner.prepare_alias_json("ops", {})

    result = runner.run("@@ops..clear", "--yes", "extra")

    assert result.returncode == 1
    assert "clear accepts only --yes" in result.stderr
    assert path.exists()


def test_clear_yes_option_is_completed_with_description(runner):
    result = runner.run(
        "__api__", "complete", "--with-descriptions", "@@ops..clear", "--"
    )

    assert result.returncode == 0
    assert result.stdout.splitlines() == ["--yes\tSkip confirmation"]


def test_clear_is_completed_with_description(runner):
    result = runner.run("__api__", "complete", "--with-descriptions", "@@ops..c")

    assert result.returncode == 0
    assert result.stdout.splitlines() == [
        "@@ops..clear\tReset this file to an empty object"
    ]


def test_resolve_rejects_clear(runner):
    result = runner.run("__api__", "resolve", "@@ops..clear")

    assert result.returncode == 1
    assert "file command" in result.stderr
