"""E2E —— $private：只保留精确调用，不进入合并空间"""

import json


def test_private_file_is_not_in_merged_lookup(runner):
    runner.prepare_alias_json("ops", {"$private": True, "deploy": "echo private"})

    merged = runner.run("deploy")
    assert merged.returncode == 1
    assert "deploy" in merged.stderr

    exact = runner.run("@@ops.deploy")
    assert exact.returncode == 0
    assert "private" in exact.stdout


def test_private_alias_mixes_with_public_siblings(runner):
    runner.prepare_alias_json(
        "ops",
        {
            "secret": {"$cmd": "echo secret", "$private": True},
            "open": "echo open",
        },
    )

    assert runner.run("secret").returncode == 1
    opened = runner.run("open")
    assert opened.returncode == 0
    assert "open" in opened.stdout

    exact = runner.run("@@ops.secret")
    assert exact.returncode == 0
    assert "secret" in exact.stdout


def test_private_group_hides_subtree_and_child_can_reexpose(runner):
    runner.prepare_alias_json(
        "ops",
        {
            "group": {
                "$private": True,
                "hidden": "echo hidden",
                "shown": {"$private": False, "deep": "echo deep"},
            }
        },
    )

    assert runner.run("group.hidden").returncode == 1
    shown = runner.run("group.shown.deep")
    assert shown.returncode == 0
    assert "deep" in shown.stdout

    exact = runner.run("@@ops.group.hidden")
    assert exact.returncode == 0
    assert "hidden" in exact.stdout


def test_private_local_file_does_not_shadow_global(runner, tmp_path):
    runner.prepare_alias_json("g", {"deploy": "echo global"})
    (tmp_path / "p.byk.json").write_text(
        json.dumps({"$private": True, "deploy": "echo local"}),
        encoding="utf-8",
    )

    merged = runner.run("deploy", cwd=str(tmp_path))
    assert merged.returncode == 0
    assert "global" in merged.stdout

    exact = runner.run("@p.deploy", cwd=str(tmp_path))
    assert exact.returncode == 0
    assert "local" in exact.stdout


def test_private_aliases_are_not_completed(runner, tmp_path):
    runner.prepare_alias_json(
        "ops",
        {
            "$private": True,
            "secret": "echo secret",
            "group": {"hidden": "echo hidden"},
            "shown": {"$private": False, "open": "echo open"},
        },
    )

    result = runner.run("__api__", "complete", "", cwd=str(tmp_path))

    assert result.returncode == 0
    assert result.stdout.splitlines() == ["@@ops", "shown"]
    assert runner.run("secret", cwd=str(tmp_path)).returncode == 1
    assert runner.run("group.hidden", cwd=str(tmp_path)).returncode == 1


def test_negative_priority_no_longer_hides_a_file(runner, tmp_path):
    (tmp_path / "p.byk.json").write_text(
        '{"$priority": -1, "hi": "echo hi"}',
        encoding="utf-8",
    )

    result = runner.run("hi", cwd=str(tmp_path))
    assert result.returncode == 0, result.stderr
    assert "hi" in result.stdout
