"""E2E —— 扫描诊断：banner 汇总、隐藏的 --check 明细与 __api__ check 契约"""

import json


def test_help_banner_stays_clean_without_problems(runner, tmp_path):
    runner.prepare_alias_json("ok", {"hi": "echo hi"})

    result = runner.run(cwd=str(tmp_path))

    assert result.returncode == 0
    assert result.stderr == ""
    lines = result.stdout.splitlines()
    assert lines[1] == "-----------------------------"
    assert lines[2].startswith("BYK v")
    assert lines[3] == "Docs https://cli.fcbyk.com"
    assert "  --check" in result.stdout
    assert "Check alias files for problems" in result.stdout


def test_help_banner_counts_problem_files(runner, tmp_path):
    runner.prepare_alias_json("ok", {"hi": "echo hi"})
    runner.prepare_alias_file("gone", '["missing.byk.json"]')

    result = runner.run(cwd=str(tmp_path))

    assert result.returncode == 0
    lines = result.stdout.splitlines()
    assert lines[1] == "-----------------------------"
    assert lines[2] == "1 alias file has problems"
    assert lines[3] == "Run byk --check for details"
    assert lines[4] == "-----------------------------"
    assert not any(line.startswith("BYK v") for line in lines)
    assert not any(line.startswith("Docs ") for line in lines)
    # 正常执行不打印扫描诊断
    assert result.stderr == ""


def test_check_clean_report(runner, tmp_path):
    runner.prepare_alias_json("ok", {"hi": "echo hi"})

    result = runner.run("--check", cwd=str(tmp_path))

    assert result.returncode == 0
    assert result.stderr == ""
    assert result.stdout == "Scanned 1 alias file; no problems.\n"


def test_check_groups_details_and_counts_levels(runner, tmp_path):
    runner.prepare_alias_file("gone", '["missing.byk.json"]')
    runner.prepare_alias_json("env", {"hi": {"$cmd": "echo hi", "$mode": "interactiv"}})

    result = runner.run("--check", cwd=str(tmp_path))

    assert result.returncode == 1
    assert "Scanned 2 alias files; 2 have problems (1 error, 1 warning)." in result.stdout
    assert "error: cannot load link target" in result.stdout
    assert "warning: $mode must be one of" in result.stdout
    assert "gone.byk.json" in result.stdout
    assert "env.byk.json" in result.stdout


def test_check_is_silent_during_normal_execution(runner, tmp_path):
    runner.prepare_alias_json("ok", {"hi": "echo still-works"})
    runner.prepare_alias_file("gone", '["missing.byk.json"]')

    result = runner.run("hi", cwd=str(tmp_path))

    assert result.returncode == 0
    assert "still-works" in result.stdout
    assert result.stderr == ""


def test_check_dedupes_files_scanned_twice(runner):
    alias_dir = runner.byk_dir / "alias"
    runner.prepare_alias_json("ok", {"hi": "echo hi"})

    result = runner.run("--check", cwd=str(alias_dir))

    assert result.returncode == 0
    assert result.stdout == "Scanned 1 alias file; no problems.\n"


def test_check_rejects_arguments(runner, tmp_path):
    result = runner.run("--check", "extra", cwd=str(tmp_path))

    assert result.returncode == 1
    assert "check takes no arguments" in result.stderr


def test_check_reports_invalid_file_name(runner, tmp_path):
    (tmp_path / "bad.name.byk.json").write_text("{}", encoding="utf-8")

    result = runner.run("--check", cwd=str(tmp_path))

    assert result.returncode == 1
    assert "Scanned 1 alias file; 1 has problems" in result.stdout
    assert "invalid file name" in result.stdout
    assert "bad.name.byk.json" in result.stdout


def test_check_reports_broken_symlink(runner, tmp_path):
    link = tmp_path / "ops.byk.json"
    link.symlink_to(tmp_path / "missing.byk.json")

    result = runner.run("--check", cwd=str(tmp_path))

    assert result.returncode == 1
    assert "Scanned 1 alias file; 1 has problems" in result.stdout
    assert "broken symlink" in result.stdout
    assert "missing.byk.json" in result.stdout


def test_check_reports_unloadable_files(runner, tmp_path):
    runner.prepare_alias_file("broken", "{not json")
    runner.prepare_alias_file("root", '"just a string"')

    result = runner.run("--check", cwd=str(tmp_path))

    assert result.returncode == 1
    assert "invalid JSON" in result.stdout
    assert "line 1" in result.stdout
    assert "root must be a JSON object" in result.stdout


def test_check_reports_alias_and_metadata_types(runner, tmp_path):
    runner.prepare_alias_json(
        "bad",
        {
            "cmd": {"$cmd": 42},
            "cwd": {"$cmd": "echo c", "$cwd": 42},
            "desc": {"$cmd": "echo d", "$description": 42},
            "group": {"$cwd": 42, "ok": "echo ok"},
        },
    )

    result = runner.run("--check", cwd=str(tmp_path))

    assert result.returncode == 1
    assert 'alias "cmd": $cmd must be a string' in result.stdout
    assert 'alias "cwd": $cwd must be a string' in result.stdout
    assert 'alias "desc": $description must be a string' in result.stdout
    assert 'group "group": $cwd must be a string' in result.stdout


def test_check_reports_file_level_metadata_types(runner, tmp_path):
    runner.prepare_alias_file(
        "meta",
        '{"$priority":"high","$private":"ture","$cwd":42,"$paths":"nope","ok":"echo ok"}',
    )

    result = runner.run("--check", cwd=str(tmp_path))

    assert result.returncode == 1
    assert "$priority must be an integer" in result.stdout
    assert '$private must be a boolean; "ture" is treated as false' in result.stdout
    assert "$cwd must be a string" in result.stdout
    assert "$paths must be an array of strings" in result.stdout


def test_check_reports_non_string_paths_entries(runner, tmp_path):
    runner.prepare_alias_file(
        "meta", '{"$paths":["./bin",42],"ok":"echo ok"}'
    )

    result = runner.run("--check", cwd=str(tmp_path))

    assert result.returncode == 1
    assert "$paths contains 1 non-string entry" in result.stdout


def test_check_reports_invalid_link_target_json(runner, tmp_path):
    target = runner.home / "proj" / "target.byk.json"
    target.parent.mkdir(parents=True, exist_ok=True)
    target.write_text("{not json", encoding="utf-8")
    runner.prepare_alias_file("link", '["../../proj/target.byk.json"]')

    result = runner.run("--check", cwd=str(tmp_path))

    assert result.returncode == 1
    assert "cannot load link target" in result.stdout
    assert "invalid JSON" in result.stdout


def test_check_findings_stay_silent_on_execution(runner, tmp_path):
    runner.prepare_alias_json("ok", {"hi": "echo hi"})
    runner.prepare_alias_file("broken", "{not json")

    result = runner.run("hi", cwd=str(tmp_path))

    assert result.returncode == 0
    assert "hi" in result.stdout
    assert result.stderr == ""


def test_check_is_completed_with_description(runner, tmp_path):
    result = runner.run(
        "__api__", "complete", "--with-descriptions", "--ch", cwd=str(tmp_path)
    )

    assert result.returncode == 0
    assert result.stdout.splitlines() == [
        "--check\tCheck alias files for problems"
    ]


def test_api_check_clean_report(runner, tmp_path):
    """无问题时返回空 items，退出码仍是 0"""
    runner.prepare_alias_json("ok", {"hi": "echo hi"})

    result = runner.run("__api__", "check", cwd=str(tmp_path))

    assert result.returncode == 0
    assert result.stderr == ""
    assert json.loads(result.stdout) == {
        "scanned": 1,
        "errors": 0,
        "warnings": 0,
        "items": [],
    }


def test_api_check_reports_items_with_absolute_paths(runner, tmp_path):
    """有问题时退出码仍为 0，细节放在 items 里，路径是绝对路径"""
    gone = runner.prepare_alias_file("gone", '["missing.byk.json"]')
    env = runner.prepare_alias_json(
        "env", {"hi": {"$cmd": "echo hi", "$mode": "activ"}}
    )

    result = runner.run("__api__", "check", cwd=str(tmp_path))

    assert result.returncode == 0, result.stderr
    payload = json.loads(result.stdout)
    assert payload["scanned"] == 2
    assert payload["errors"] == 1
    assert payload["warnings"] == 1

    by_path = {item["path"]: item for item in payload["items"]}
    assert by_path[str(gone)]["level"] == "error"
    assert "cannot load link target" in by_path[str(gone)]["message"]
    assert by_path[str(env)]["level"] == "warning"
    assert "$mode must be one of" in by_path[str(env)]["message"]


def test_api_check_rejects_arguments(runner, tmp_path):
    result = runner.run("__api__", "check", "extra", cwd=str(tmp_path))

    assert result.returncode == 1
    assert "takes no arguments" in result.stderr
    assert result.stdout == ""
