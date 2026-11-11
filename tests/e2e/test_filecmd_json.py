import json
import os

import pytest


RAW = '{\n  "z": "echo z",\n  "a": "echo a"\n}\n'


def test_json_outputs_raw_content_to_stdout(runner):
    runner.prepare_alias_file("ops", RAW)

    result = runner.run("@@ops..json")

    assert result.returncode == 0
    assert result.stdout == RAW
    assert result.stderr == ""


def test_json_local_and_anonymous(runner, tmp_path):
    (tmp_path / "ops.byk.json").write_text('{"a": "echo a"}\n', encoding="utf-8")
    runner.prepare_alias_file("", '{"b": "echo b"}\n')

    local = runner.run("@ops..json", cwd=str(tmp_path))
    anonymous = runner.run("@@..json")

    assert local.returncode == 0
    assert local.stdout == '{"a": "echo a"}\n'
    assert anonymous.returncode == 0
    assert anonymous.stdout == '{"b": "echo b"}\n'


def test_json_does_not_add_trailing_newline(runner):
    content = '{"a": "echo a"}'
    runner.prepare_alias_file("ops", content)

    result = runner.run("@@ops..json")

    assert result.returncode == 0
    assert result.stdout == content


def test_json_does_not_follow_import_index(runner):
    runner.prepare_alias_file("shared", "not json at all")
    index = '["shared.byk.json"]\n'
    runner.prepare_alias_file("ops", index)

    result = runner.run("@@ops..json")

    assert result.returncode == 0
    assert result.stdout == index


def test_json_expand_substitutes_vars_and_removes_var_block(runner):
    runner.prepare_alias_file(
        "ops",
        '{"$var":{"TOOLS":"${byk_dir}/tools"},'
        '"build":"cd ${file_dir} && ${TOOLS}/build.sh"}',
    )

    result = runner.run("@@ops..json", "--expand")

    assert result.returncode == 0
    assert result.stderr == ""
    assert result.stdout.endswith("\n")
    data = json.loads(result.stdout)
    assert "$var" not in data
    assert data["build"] == (
        f"cd {runner.byk_dir / 'alias'} && {runner.byk_dir / 'tools'}/build.sh"
    )


def test_json_expand_reports_variable_warnings(runner):
    runner.prepare_alias_file("ops", '{"$var":{"A":"${A}x"},"run":"echo ${A}"}')

    result = runner.run("@@ops..json", "--expand")

    assert result.returncode == 0
    assert json.loads(result.stdout)["run"] == "echo ${A}x"
    assert "Warning" in result.stderr
    assert "references itself" in result.stderr


def test_json_expand_does_not_follow_import_index(runner):
    runner.prepare_alias_file("shared", "not json at all")
    runner.prepare_alias_file("ops", '["${byk_dir}/shared.byk.json"]')

    result = runner.run("@@ops..json", "--expand")

    assert result.returncode == 0
    assert json.loads(result.stdout) == [f"{runner.byk_dir}/shared.byk.json"]


def test_json_expand_invalid_json_fails_without_output(runner):
    runner.prepare_alias_file("ops", "{not json")

    result = runner.run("@@ops..json", "--expand")

    assert result.returncode == 1
    assert result.stdout == ""
    assert "Invalid JSON" in result.stderr
    assert "line 1" in result.stderr


def test_json_no_check_outputs_invalid_json(runner):
    content = "{not json"
    runner.prepare_alias_file("ops", content)

    result = runner.run("@@ops..json", "--no-check")

    assert result.returncode == 0
    assert result.stdout == content
    assert result.stderr == ""


def test_json_no_check_and_expand_conflict(runner):
    runner.prepare_alias_file("ops", "{}")

    result = runner.run("@@ops..json", "--expand", "--no-check")

    assert result.returncode == 1
    assert result.stdout == ""
    assert "--expand and --no-check cannot be combined" in result.stderr


def test_json_invalid_json_fails_without_output(runner):
    runner.prepare_alias_file("ops", "{not json")

    result = runner.run("@@ops..json")

    assert result.returncode == 1
    assert result.stdout == ""
    assert "Invalid JSON" in result.stderr
    assert "line 1" in result.stderr


def test_json_missing_file_fails(runner):
    result = runner.run("@@ops..json")

    assert result.returncode == 1
    assert result.stdout == ""
    assert "File not found" in result.stderr


def test_json_hints_other_scope(runner, tmp_path):
    (tmp_path / "ops.byk.json").write_text("{}", encoding="utf-8")

    result = runner.run("@@ops..json", cwd=str(tmp_path))

    assert result.returncode == 1
    assert "use @ops..json" in result.stderr


def test_json_refuses_directory(runner):
    path = runner.byk_dir / "alias" / "ops.byk.json"
    path.mkdir(parents=True)

    result = runner.run("@@ops..json")

    assert result.returncode == 1
    assert result.stdout == ""
    assert "Refusing to read directory" in result.stderr


@pytest.mark.skipif(os.name == "nt", reason="symlink creation may need privileges")
def test_json_follows_symlink(runner):
    real = runner.prepare_alias_file("real", '{"a": "echo a"}\n')
    link = runner.byk_dir / "alias" / "ops.byk.json"
    link.symlink_to(real)

    result = runner.run("@@ops..json")

    assert result.returncode == 0
    assert result.stdout == '{"a": "echo a"}\n'


def test_json_rejects_arguments(runner):
    runner.prepare_alias_file("ops", "{}")

    result = runner.run("@@ops..json", "--pretty")

    assert result.returncode == 1
    assert result.stdout == ""
    assert "json accepts only --expand or --no-check" in result.stderr


def test_json_command_is_completed(runner):
    result = runner.run("__api__", "complete", "@@ops..")

    assert result.returncode == 0
    assert "@@ops..json" in result.stdout.splitlines()


def test_json_options_are_completed(runner):
    both = runner.run("__api__", "complete", "@@ops..json", "--")
    expand = runner.run("__api__", "complete", "@@ops..json", "--e")
    no_check = runner.run("__api__", "complete", "@@ops..json", "--n")

    assert both.stdout.splitlines() == ["--expand", "--no-check"]
    assert expand.stdout.splitlines() == ["--expand"]
    assert no_check.stdout.splitlines() == ["--no-check"]


def test_json_options_are_completed_with_descriptions(runner):
    result = runner.run(
        "__api__", "complete", "--with-descriptions", "@@ops..json", "--"
    )

    assert result.stdout.splitlines() == [
        "--expand\tExpand file variables and pretty-print",
        "--no-check\tSkip JSON validation and print raw",
    ]


def test_json_options_are_not_offered_without_dash(runner):
    result = runner.run("__api__", "complete", "@@ops..json", "")

    assert result.returncode == 0
    assert result.stdout == ""


def test_resolve_rejects_json_without_output(runner):
    runner.prepare_alias_file("ops", RAW)

    result = runner.run("__api__", "resolve", "@@ops..json")

    assert result.returncode == 1
    assert result.stdout == ""
    assert "file command" in result.stderr
