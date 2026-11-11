import os

import pytest


def test_destroy_yes_removes_global_file(runner):
    path = runner.prepare_alias_json("ops", {"build": "echo hi"})

    result = runner.run("@@ops..destroy", "--yes")

    assert result.returncode == 0
    assert result.stdout == ""
    assert "Removed" in result.stderr
    assert "Type" not in result.stderr
    assert not path.exists()


def test_destroy_yes_removes_local_file(runner, tmp_path):
    path = tmp_path / "ops.byk.json"
    path.write_text('{"build": "echo hi"}', encoding="utf-8")

    result = runner.run("@ops..destroy", "--yes", cwd=str(tmp_path))

    assert result.returncode == 0
    assert not path.exists()
    assert not runner.byk_dir.exists()


def test_destroy_yes_removes_anonymous_file(runner):
    path = runner.prepare_alias_file("", "{}")

    result = runner.run("@@..destroy", "--yes")

    assert result.returncode == 0
    assert not path.exists()


def test_destroy_yes_missing_file_fails(runner):
    result = runner.run("@@ops..destroy", "--yes")

    assert result.returncode == 1
    assert "File not found" in result.stderr


def test_destroy_refuses_directory(runner):
    path = runner.byk_dir / "alias" / "ops.byk.json"
    path.mkdir(parents=True)

    result = runner.run("@@ops..destroy", "--yes")

    assert result.returncode == 1
    assert "Refusing to remove directory" in result.stderr
    assert path.is_dir()


def test_destroy_non_tty_requires_yes(runner):
    path = runner.prepare_alias_json("ops", {})

    result = runner.run("@@ops..destroy")

    assert result.returncode == 1
    assert "--yes" in result.stderr
    assert path.exists()


def test_destroy_interactive_requires_matching_stem(runner):
    path = runner.prepare_alias_json(
        "ops", {"build": "echo build", "group": {"test": "echo test"}}
    )

    result = runner.run("@@ops..destroy", input="ops\n", tty_stderr=True)

    assert result.returncode == 0
    assert "This will permanently remove:" in result.stderr
    assert "(2 aliases)" in result.stderr
    assert "Type" in result.stderr
    assert "ops:" in result.stderr
    assert not path.exists()


def test_destroy_interactive_wrong_input_cancels(runner):
    path = runner.prepare_alias_json("ops", {})

    result = runner.run("@@ops..destroy", input="nope\n", tty_stderr=True)

    assert result.returncode == 0
    assert "Confirmation does not match. Cancelled." in result.stderr
    assert path.exists()


def test_destroy_interactive_eof_cancels(runner):
    path = runner.prepare_alias_json("ops", {})

    result = runner.run("@@ops..destroy", input="", tty_stderr=True)

    assert result.returncode == 0
    assert "Cancelled." in result.stderr
    assert path.exists()


def test_destroy_anonymous_interactive_token(runner):
    path = runner.prepare_alias_file("", "{}")

    result = runner.run("@@..destroy", input=".byk.json\n", tty_stderr=True)

    assert result.returncode == 0
    assert ".byk.json:" in result.stderr
    assert not path.exists()


def test_destroy_import_index_keeps_target(runner):
    target = runner.prepare_alias_json("shared", {"build": "echo hi"})
    index = runner.prepare_alias_file("ops", '["shared.byk.json"]')

    result = runner.run("@@ops..destroy", "--yes")

    assert result.returncode == 0
    assert not index.exists()
    assert target.exists()


@pytest.mark.skipif(os.name == "nt", reason="symlink creation may need privileges")
def test_destroy_symlink_removes_link_only(runner):
    real = runner.prepare_alias_json("real", {"build": "echo hi"})
    link = runner.byk_dir / "alias" / "ops.byk.json"
    link.symlink_to(real)

    result = runner.run("@@ops..destroy", "--yes")

    assert result.returncode == 0
    assert "Removed symlink" in result.stderr
    assert not os.path.lexists(link)
    assert real.exists()


def test_destroy_hints_other_scope(runner, tmp_path):
    (tmp_path / "ops.byk.json").write_text("{}", encoding="utf-8")

    result = runner.run("@@ops..destroy", "--yes", cwd=str(tmp_path))

    assert result.returncode == 1
    assert "use @ops..destroy" in result.stderr


def test_destroy_rejects_extra_arguments(runner):
    path = runner.prepare_alias_json("ops", {})

    result = runner.run("@@ops..destroy", "--yes", "extra")

    assert result.returncode == 1
    assert "accepts only --yes" in result.stderr
    assert path.exists()


def test_destroy_command_still_completes(runner):
    result = runner.run("__api__", "complete", "@@ops..")

    assert result.returncode == 0
    assert "@@ops..destroy" in result.stdout.splitlines()


def test_destroy_yes_option_is_completed(runner):
    result = runner.run("__api__", "complete", "@@ops..destroy", "--")

    assert result.returncode == 0
    assert result.stdout.splitlines() == ["--yes"]


def test_destroy_yes_option_is_completed_with_description(runner):
    result = runner.run(
        "__api__", "complete", "--with-descriptions", "@@ops..destroy", "--"
    )

    assert result.returncode == 0
    assert result.stdout.splitlines() == ["--yes\tSkip confirmation"]


def test_resolve_rejects_destroy_without_deleting(runner):
    path = runner.prepare_alias_json("ops", {})

    result = runner.run("__api__", "resolve", "@@ops..destroy")

    assert result.returncode == 1
    assert "file command" in result.stderr
    assert path.exists()
