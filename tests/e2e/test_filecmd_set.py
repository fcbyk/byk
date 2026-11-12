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


def test_set_marks_alias_private(runner):
    path = runner.prepare_alias_json("ops", {"build": "echo hi"})

    result = runner.run("@@ops..set", "build", "--private")

    assert result.returncode == 0
    assert read(path) == {"build": {"$cmd": "echo hi", "$private": True}}


def test_set_no_private_reexposes_alias(runner):
    path = runner.prepare_alias_json(
        "ops", {"group": {"$private": True, "build": "echo hi"}}
    )

    result = runner.run("@@ops..set", "group.build", "--no-private")

    assert result.returncode == 0
    assert read(path) == {
        "group": {
            "$private": True,
            "build": {"$cmd": "echo hi", "$private": False},
        }
    }


def test_set_private_removes_alias_from_merged_lookup(runner):
    runner.prepare_alias_json("ops", {"build": "echo hi"})
    assert runner.run("build").returncode == 0

    result = runner.run("@@ops..set", "build", "--private")

    assert result.returncode == 0
    assert runner.run("build").returncode == 1
    exact = runner.run("@@ops.build")
    assert exact.returncode == 0
    assert "hi" in exact.stdout


def test_set_root_private_and_priority_clamp(runner):
    path = runner.prepare_alias_json("ops", {"build": "echo hi"})

    result = runner.run("@@ops..set", "--private", "--priority", "-1")

    assert result.returncode == 0
    assert read(path) == {"$priority": 0, "$private": True, "build": "echo hi"}

    result = runner.run("@@ops..set", "--no-private")

    assert result.returncode == 0
    assert read(path) == {"$priority": 0, "build": "echo hi"}


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


def test_set_root_metadata_without_key(runner):
    path = runner.prepare_alias_json("ops", {"build": "echo hi"})

    result = runner.run(
        "@@ops..set", "--priority", "10", "--cwd", "./src", "--mode", "silent"
    )

    assert result.returncode == 0
    assert "Set file metadata in" in result.stderr
    assert read(path) == {
        "$priority": 10,
        "$cwd": "./src",
        "$mode": "silent",
        "build": "echo hi",
    }


def test_set_root_unset_metadata(runner):
    path = runner.prepare_alias_json(
        "ops",
        {"$priority": -1, "$cwd": "./src", "$mode": "silent", "build": "echo hi"},
    )

    result = runner.run("@@ops..set", "--unset", "priority,cwd,mode")

    assert result.returncode == 0
    assert read(path) == {"build": "echo hi"}


def test_set_root_rejects_alias_flags(runner):
    path = runner.prepare_alias_json("ops", {"build": "echo hi"})

    cases = [
        (("--cmd", "echo x"), "File root has no command"),
        (("--description", "x"), "--description requires an alias key"),
        (("--recursive",), "--recursive requires an alias key"),
        (("--unset", "description"), "File root supports only"),
    ]
    for extra, expected in cases:
        result = runner.run("@@ops..set", *extra)
        assert result.returncode == 1
        assert expected in result.stderr

    assert read(path) == {"build": "echo hi"}


def test_set_priority_requires_root_mode(runner):
    path = runner.prepare_alias_json("ops", {"build": "echo hi"})

    result = runner.run("@@ops..set", "build", "--priority", "1")

    assert result.returncode == 1
    assert "only applies to the file root" in result.stderr
    assert read(path) == {"build": "echo hi"}


def test_set_rejects_invalid_priority(runner):
    path = runner.prepare_alias_json("ops", {"build": "echo hi"})

    result = runner.run("@@ops..set", "--priority", "high")

    assert result.returncode == 1
    assert "Invalid priority" in result.stderr
    assert read(path) == {"build": "echo hi"}


def test_set_root_requires_change(runner):
    path = runner.prepare_alias_json("ops", {"build": "echo hi"})

    result = runner.run("@@ops..set")

    assert result.returncode == 1
    assert "requires an alias key or a metadata change" in result.stderr
    assert read(path) == {"build": "echo hi"}


def test_set_root_paths_replaces_existing_list(runner):
    path = runner.prepare_alias_json(
        "ops", {"$paths": ["./old"], "build": "echo hi"}
    )

    result = runner.run(
        "@@ops..set", "--paths", "./bin", "--paths", "~/tools/bin"
    )

    assert result.returncode == 0
    assert "Set file metadata in" in result.stderr
    assert read(path) == {
        "$paths": ["./bin", "~/tools/bin"],
        "build": "echo hi",
    }


def test_set_root_unsets_paths(runner):
    path = runner.prepare_alias_json(
        "ops", {"$paths": ["./bin"], "build": "echo hi"}
    )

    result = runner.run("@@ops..set", "--unset", "paths")

    assert result.returncode == 0
    assert read(path) == {"build": "echo hi"}


def test_set_paths_requires_root_mode(runner):
    path = runner.prepare_alias_json("ops", {"build": "echo hi"})

    result = runner.run("@@ops..set", "build", "--paths", "./bin")

    assert result.returncode == 1
    assert "only applies to the file root" in result.stderr
    assert read(path) == {"build": "echo hi"}


def test_set_rejects_empty_path(runner):
    path = runner.prepare_alias_json("ops", {"build": "echo hi"})

    result = runner.run("@@ops..set", "--paths", "")

    assert result.returncode == 1
    assert "requires a non-empty value" in result.stderr
    assert read(path) == {"build": "echo hi"}


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
        "--private\tKeep this file or alias out of merged lookup",
        "--no-private\tExpose it to merged lookup again",
        "--unset\tRemove metadata fields",
        "--priority\tSet the file priority (omit key)",
        "--paths\tPrepend directories to PATH (omit key)",
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
        "--private",
        "--no-private",
        "--unset",
        "--priority",
        "--paths",
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
