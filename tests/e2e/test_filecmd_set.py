import json


def read(path):
    return json.loads(path.read_text(encoding="utf-8"))


def test_set_creates_string_alias(runner):
    path = runner.prepare_alias_file("ops", "{}\n")

    result = runner.run("@@ops..set", "build", "--cmd", "echo hi")

    assert result.returncode == 0
    assert result.stdout == ""
    assert "Set build in" in result.stderr
    assert read(path) == {"build": "echo hi"}


def test_set_creates_nested_alias(runner):
    path = runner.prepare_alias_file("ops", "{}\n")

    result = runner.run("@@ops..set", "web.deploy", "--cmd", "echo hi")

    assert result.returncode == 0
    assert read(path) == {"web": {"deploy": "echo hi"}}


def test_set_creates_meta_alias(runner):
    path = runner.prepare_alias_file("ops", "{}\n")

    result = runner.run(
        "@@ops..set",
        "build",
        "--cmd",
        "npm run build",
        "--cwd",
        "./web",
        "--mode",
        "silent",
        "--description",
        "Build the web app",
        "--recursive",
    )

    assert result.returncode == 0
    assert read(path) == {
        "build": {
            "$cmd": "npm run build",
            "$cwd": "./web",
            "$mode": "silent",
            "$description": "Build the web app",
            "$recursive": True,
        }
    }


def test_set_updates_string_alias(runner):
    path = runner.prepare_alias_json("ops", {"build": "echo old"})

    result = runner.run("@@ops..set", "build", "--cmd", "echo new")

    assert result.returncode == 0
    assert read(path) == {"build": "echo new"}


def test_set_preserves_metadata_when_updating_command(runner):
    path = runner.prepare_alias_json(
        "ops", {"build": {"$cmd": "echo old", "$description": "keep"}}
    )

    result = runner.run("@@ops..set", "build", "--cmd", "echo new")

    assert result.returncode == 0
    assert read(path) == {"build": {"$cmd": "echo new", "$description": "keep"}}


def test_set_converts_string_alias_to_meta(runner):
    path = runner.prepare_alias_json("ops", {"build": "echo hi"})

    result = runner.run("@@ops..set", "build", "--cwd", "./web")

    assert result.returncode == 0
    assert read(path) == {"build": {"$cmd": "echo hi", "$cwd": "./web"}}


def test_set_group_metadata(runner):
    path = runner.prepare_alias_json("ops", {"web": {"deploy": "echo hi"}})

    result = runner.run(
        "@@ops..set", "web", "--cwd", ".", "--mode", "interactive"
    )

    assert result.returncode == 0
    assert read(path) == {
        "web": {
            "$cwd": ".",
            "$mode": "interactive",
            "deploy": "echo hi",
        }
    }


def test_set_unset_collapses_back_to_string(runner):
    path = runner.prepare_alias_json(
        "ops", {"build": {"$cmd": "echo hi", "$mode": "silent"}}
    )

    result = runner.run("@@ops..set", "build", "--unset", "mode")

    assert result.returncode == 0
    assert read(path) == {"build": "echo hi"}


def test_set_no_recursive_removes_flag(runner):
    path = runner.prepare_alias_json(
        "ops", {"build": {"$cmd": "echo hi", "$recursive": True}}
    )

    result = runner.run("@@ops..set", "build", "--no-recursive")

    assert result.returncode == 0
    assert read(path) == {"build": "echo hi"}


def test_set_accepts_command_starting_with_dash(runner):
    path = runner.prepare_alias_file("ops", "{}\n")

    result = runner.run("@@ops..set", "version", "--cmd", "--version")

    assert result.returncode == 0
    assert read(path) == {"version": "--version"}


def test_set_refuses_link_file(runner):
    target = runner.prepare_alias_json("shared", {"build": "echo hi"})
    link_file = runner.prepare_alias_file("ops", '["shared.byk.json"]')

    result = runner.run("@@ops..set", "build", "--cmd", "echo new")

    assert result.returncode == 1
    assert "link file" in result.stderr
    assert link_file.read_text(encoding="utf-8") == '["shared.byk.json"]'
    assert read(target) == {"build": "echo hi"}


def test_set_missing_file_hints_init(runner):
    result = runner.run("@@ops..set", "build", "--cmd", "echo hi")

    assert result.returncode == 1
    assert "File not found" in result.stderr
    assert "@@ops..init" in result.stderr


def test_set_invalid_json(runner):
    path = runner.prepare_alias_file("ops", "{not json")

    result = runner.run("@@ops..set", "build", "--cmd", "echo hi")

    assert result.returncode == 1
    assert "Failed to parse" in result.stderr
    assert path.read_text(encoding="utf-8") == "{not json"


def test_set_rejects_invalid_key(runner):
    path = runner.prepare_alias_file("ops", "{}\n")

    result = runner.run("@@ops..set", "a..b", "--cmd", "echo hi")

    assert result.returncode == 1
    assert "empty segment" in result.stderr
    assert read(path) == {}


def test_set_new_alias_requires_command(runner):
    path = runner.prepare_alias_file("ops", "{}\n")

    result = runner.run("@@ops..set", "build", "--cwd", ".")

    assert result.returncode == 1
    assert "requires --cmd" in result.stderr
    assert read(path) == {}


def test_set_rejects_set_and_unset_conflict(runner):
    path = runner.prepare_alias_json("ops", {"build": "echo hi"})

    result = runner.run(
        "@@ops..set", "build", "--cwd", ".", "--unset", "cwd"
    )

    assert result.returncode == 1
    assert "Cannot set and unset the same field" in result.stderr
    assert read(path) == {"build": "echo hi"}


def test_set_rejects_unknown_option(runner):
    path = runner.prepare_alias_file("ops", "{}\n")

    result = runner.run("@@ops..set", "build", "--bogus")

    assert result.returncode == 1
    assert "Unknown option: --bogus" in result.stderr
    assert read(path) == {}


def test_set_local_and_anonymous(runner, tmp_path):
    local = tmp_path / "ops.byk.json"
    local.write_text("{}\n", encoding="utf-8")
    anonymous = runner.prepare_alias_file("", "{}\n")

    local_result = runner.run(
        "@ops..set", "build", "--cmd", "echo local", cwd=str(tmp_path)
    )
    anonymous_result = runner.run("@@..set", "build", "--cmd", "echo anon")

    assert local_result.returncode == 0
    assert read(local) == {"build": "echo local"}
    assert anonymous_result.returncode == 0
    assert read(anonymous) == {"build": "echo anon"}


def test_set_is_completed_with_description(runner):
    result = runner.run("__api__", "complete", "--with-descriptions", "@@ops..se")

    assert result.returncode == 0
    assert result.stdout.splitlines() == [
        "@@ops..set\tCreate or update an alias"
    ]


def test_set_options_are_completed_with_descriptions(runner):
    result = runner.run(
        "__api__", "complete", "--with-descriptions", "@@ops..set", "--"
    )

    assert result.returncode == 0
    assert result.stdout.splitlines() == [
        "--cmd\tSet the alias command",
        "--cwd\tSet the working directory",
        "--mode\tSet the execution mode",
        "--description\tSet the description",
        "--recursive\tAllow re-entry on the call chain",
        "--no-recursive\tRemove the recursive flag",
        "--unset\tRemove metadata fields",
    ]


def test_set_options_are_completed_after_key(runner):
    result = runner.run("__api__", "complete", "@@ops..set", "build", "--")

    assert result.returncode == 0
    assert result.stdout.splitlines() == [
        "--cmd",
        "--cwd",
        "--mode",
        "--description",
        "--recursive",
        "--no-recursive",
        "--unset",
    ]


def test_set_options_are_completed_after_key_with_descriptions(runner):
    result = runner.run(
        "__api__", "complete", "--with-descriptions", "@@ops..set", "build", "--c"
    )

    assert result.returncode == 0
    assert result.stdout.splitlines() == [
        "--cmd\tSet the alias command",
        "--cwd\tSet the working directory",
    ]


def test_resolve_rejects_set(runner):
    result = runner.run("__api__", "resolve", "@@ops..set")

    assert result.returncode == 1
    assert "file command" in result.stderr
