import json
import os


def test_init_global_creates_dir_and_file(runner):
    result = runner.run("@@ops", "init")

    assert result.returncode == 0
    assert result.stdout == ""
    assert "Created" in result.stderr
    assert (runner.byk_dir / "alias" / "ops.byk.json").read_text() == "{}\n"


def test_init_global_existing_file_is_not_overwritten(runner):
    path = runner.prepare_alias_file("ops", '{"keep": true}')

    result = runner.run("@@ops", "init")

    assert result.returncode == 1
    assert "already exists" in result.stderr.lower()
    assert path.read_text() == '{"keep": true}'


def test_init_local_uses_cwd_without_creating_home(runner, tmp_path):
    result = runner.run("@ops", "init", cwd=str(tmp_path))

    assert result.returncode == 0
    assert (tmp_path / "ops.byk.json").read_text() == "{}\n"
    assert not runner.byk_dir.exists()


def test_init_anonymous_files(runner, tmp_path):
    global_result = runner.run("@@", "init", cwd=str(tmp_path))
    local_result = runner.run("@", "init", cwd=str(tmp_path))

    assert global_result.returncode == 0
    assert local_result.returncode == 0
    assert (runner.byk_dir / "alias" / ".byk.json").read_text() == "{}\n"
    assert (tmp_path / ".byk.json").read_text() == "{}\n"


def test_init_link_accepts_global_name(runner):
    source = runner.prepare_alias_json("src", {"build": "echo hi"})

    result = runner.run("@@ops", "init", "--link", "@@src")

    assert result.returncode == 0
    path = runner.byk_dir / "alias" / "ops.byk.json"
    assert json.loads(path.read_text()) == [os.path.realpath(source)]


def test_init_rejects_arguments(runner):
    result = runner.run("@@ops", "init", "extra")

    assert result.returncode == 1
    assert "accepts only --link" in result.stderr
    assert not (runner.byk_dir / "alias" / "ops.byk.json").exists()


def test_init_rejects_invalid_name(runner, tmp_path):
    result = runner.run("@a/b", "init", cwd=str(tmp_path))

    assert result.returncode == 1
    assert "Invalid file name" in result.stderr


def test_unknown_verb_lists_available_commands_and_alias_hint(runner):
    result = runner.run("@@ops", "explode")

    assert result.returncode == 1
    assert 'Unknown file command: "explode"' in result.stderr
    assert "init" in result.stderr
    assert "Hint: to run an alias from this file, use @@ops.<key>" in result.stderr


def test_exact_alias_with_single_dot_still_works(runner):
    runner.prepare_alias_json("ops", {"init": "echo user-init"})

    result = runner.run("@@ops.init")

    assert result.returncode == 0
    assert "user-init" in result.stdout


def test_bare_file_token_prints_file_command_help(runner):
    result = runner.run("@@ops")

    assert result.returncode == 0
    assert result.stderr == ""
    assert "File commands:" in result.stdout
    assert any(line.startswith("  @@ops init") for line in result.stdout.splitlines())
    assert "Unrecognized" not in result.stderr


def test_help_flag_matches_help_verb(runner):
    long_flag = runner.run("@@ops", "--help")
    short_flag = runner.run("@@ops", "-h")
    verb = runner.run("@@ops", "help")

    assert long_flag.returncode == 0
    assert long_flag.stdout == verb.stdout
    assert short_flag.returncode == 0
    assert short_flag.stdout == verb.stdout


def test_complete_offers_verbs_after_file_token(runner):
    runner.prepare_alias_json("ops", {"build": "echo build"})

    verbs = runner.run("__api__", "complete", "@@ops", "")
    dotted = runner.run("__api__", "complete", "@@ops.")

    assert verbs.returncode == 0
    assert "init" in verbs.stdout.splitlines()
    assert "build" not in verbs.stdout.splitlines()
    assert dotted.returncode == 0
    assert "@@ops.build" in dotted.stdout.splitlines()
    assert "@@ops.init" not in dotted.stdout


def test_resolve_rejects_file_command_without_creating_file(runner):
    result = runner.run("__api__", "resolve", "@@ops", "init")

    assert result.returncode == 1
    assert "file command" in result.stderr
    assert not (runner.byk_dir / "alias" / "ops.byk.json").exists()


def test_help_does_not_mention_file_commands(runner):
    result = runner.run("-h")

    assert result.returncode == 0
    assert "File commands:" not in result.stdout
