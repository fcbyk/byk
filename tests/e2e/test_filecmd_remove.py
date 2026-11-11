import json


def read(path):
    return json.loads(path.read_text(encoding="utf-8"))


def test_remove_leaf_keeps_others(runner):
    path = runner.prepare_alias_json(
        "ops", {"build": "echo hi", "test": "echo test"}
    )

    result = runner.run("@@ops..remove", "build")

    assert result.returncode == 0
    assert result.stdout == ""
    assert "Removed build from" in result.stderr
    assert read(path) == {"test": "echo test"}


def test_remove_nested_prunes_empty_group(runner):
    path = runner.prepare_alias_json("ops", {"web": {"deploy": "echo hi"}})

    result = runner.run("@@ops..remove", "web.deploy")

    assert result.returncode == 0
    assert read(path) == {}


def test_remove_nested_prunes_group_metadata(runner):
    path = runner.prepare_alias_json(
        "ops", {"web": {"$cwd": ".", "deploy": "echo hi"}}
    )

    result = runner.run("@@ops..remove", "web.deploy")

    assert result.returncode == 0
    assert read(path) == {}


def test_remove_keeps_parent_with_command(runner):
    path = runner.prepare_alias_json(
        "ops", {"web": {"$cmd": "echo web", "deploy": "echo hi"}}
    )

    result = runner.run("@@ops..remove", "web.deploy")

    assert result.returncode == 0
    assert read(path) == {"web": {"$cmd": "echo web"}}


def test_remove_whole_group(runner):
    path = runner.prepare_alias_json(
        "ops",
        {
            "web": {"$cwd": ".", "deploy": "echo hi", "test": "echo test"},
            "build": "echo build",
        },
    )

    result = runner.run("@@ops..remove", "web")

    assert result.returncode == 0
    assert read(path) == {"build": "echo build"}


def test_remove_alias_object_with_metadata(runner):
    path = runner.prepare_alias_json(
        "ops",
        {
            "build": {"$cmd": "echo hi", "$description": "keep"},
            "test": "echo test",
        },
    )

    result = runner.run("@@ops..remove", "build")

    assert result.returncode == 0
    assert read(path) == {"test": "echo test"}


def test_remove_missing_alias(runner):
    path = runner.prepare_alias_json("ops", {"build": "echo hi"})

    result = runner.run("@@ops..remove", "deploy")

    assert result.returncode == 1
    assert "Alias not found: deploy" in result.stderr
    assert read(path) == {"build": "echo hi"}


def test_remove_missing_intermediate(runner):
    runner.prepare_alias_json("ops", {"build": "echo hi"})

    result = runner.run("@@ops..remove", "web.deploy")

    assert result.returncode == 1
    assert "Alias not found: web.deploy" in result.stderr


def test_remove_refuses_non_object_intermediate(runner):
    path = runner.prepare_alias_json("ops", {"web": "echo hi"})

    result = runner.run("@@ops..remove", "web.deploy")

    assert result.returncode == 1
    assert "Cannot remove below non-object alias: web" in result.stderr
    assert read(path) == {"web": "echo hi"}


def test_remove_refuses_link_file(runner):
    target = runner.prepare_alias_json("shared", {"build": "echo hi"})
    link_file = runner.prepare_alias_file("ops", '["shared.byk.json"]')

    result = runner.run("@@ops..remove", "build")

    assert result.returncode == 1
    assert "link file" in result.stderr
    assert link_file.read_text(encoding="utf-8") == '["shared.byk.json"]'
    assert read(target) == {"build": "echo hi"}


def test_remove_missing_file_has_no_init_hint(runner):
    result = runner.run("@@ops..remove", "build")

    assert result.returncode == 1
    assert "File not found" in result.stderr
    assert "Hint:" not in result.stderr


def test_remove_invalid_json(runner):
    path = runner.prepare_alias_file("ops", "{not json")

    result = runner.run("@@ops..remove", "build")

    assert result.returncode == 1
    assert "Failed to parse" in result.stderr
    assert path.read_text(encoding="utf-8") == "{not json"


def test_remove_rejects_metadata_key_with_unset_hint(runner):
    path = runner.prepare_alias_json("ops", {"build": "echo hi"})

    result = runner.run("@@ops..remove", "build.$cwd")

    assert result.returncode == 1
    assert "metadata field" in result.stderr
    assert "--unset" in result.stderr
    assert read(path) == {"build": "echo hi"}


def test_remove_rejects_extra_keys(runner):
    path = runner.prepare_alias_json("ops", {"build": "echo hi"})

    result = runner.run("@@ops..remove", "build", "test")

    assert result.returncode == 1
    assert "remove takes exactly one key" in result.stderr
    assert read(path) == {"build": "echo hi"}


def test_remove_local_and_anonymous(runner, tmp_path):
    local = tmp_path / "ops.byk.json"
    local.write_text('{"build": "echo local"}', encoding="utf-8")
    anonymous = runner.prepare_alias_json("", {"build": "echo anon"})

    local_result = runner.run(
        "@ops..remove", "build", cwd=str(tmp_path)
    )
    anonymous_result = runner.run("@@..remove", "build")

    assert local_result.returncode == 0
    assert read(local) == {}
    assert anonymous_result.returncode == 0
    assert read(anonymous) == {}


def test_remove_key_completion_lists_keys_and_groups(runner):
    runner.prepare_alias_json(
        "ops", {"build": "echo hi", "web": {"deploy": "echo d"}}
    )

    result = runner.run("__api__", "complete", "@@ops..remove", "")

    assert result.returncode == 0
    assert result.stdout.splitlines() == ["build", "web", "web.deploy"]


def test_remove_key_completion_filters_nested_paths(runner):
    runner.prepare_alias_json(
        "ops", {"build": "echo hi", "web": {"deploy": "echo d"}}
    )

    result = runner.run("__api__", "complete", "@@ops..remove", "web.")

    assert result.returncode == 0
    assert result.stdout.splitlines() == ["web.deploy"]


def test_remove_key_completion_excludes_metadata(runner):
    runner.prepare_alias_json(
        "ops", {"build": {"$cmd": "echo hi", "$cwd": "."}}
    )

    result = runner.run("__api__", "complete", "@@ops..remove", "$")

    assert result.returncode == 0
    assert result.stdout == ""


def test_remove_key_completion_stops_after_key(runner):
    runner.prepare_alias_json("ops", {"build": "echo hi"})

    result = runner.run("__api__", "complete", "@@ops..remove", "build", "")

    assert result.returncode == 0
    assert result.stdout == ""


def test_help_for_remove_shows_usage(runner):
    result = runner.run("@@ops..help", "remove")

    assert result.returncode == 0
    assert result.stdout.startswith("\n@@ops..remove <key>\n")
    assert "Remove an alias or group" in result.stdout
    assert "Options:" not in result.stdout


def test_resolve_rejects_remove(runner):
    result = runner.run("__api__", "resolve", "@@ops..remove")

    assert result.returncode == 1
    assert "file command" in result.stderr
