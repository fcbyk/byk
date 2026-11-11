import os


def path_line(result):
    assert result.returncode == 0
    assert result.stderr == ""
    assert result.stdout.endswith("\n")
    assert not result.stdout.endswith("\n\n")
    line = result.stdout.removesuffix("\n")
    assert os.path.isabs(line)
    return line


def test_path_global_named(runner):
    result = runner.run("@@ops..path")

    line = path_line(result)
    expected = runner.byk_dir / "alias" / "ops.byk.json"
    assert os.path.realpath(line) == os.path.realpath(expected)


def test_path_local_named(runner, tmp_path):
    result = runner.run("@ops..path", cwd=str(tmp_path))

    line = path_line(result)
    expected = tmp_path / "ops.byk.json"
    assert os.path.realpath(line) == os.path.realpath(expected)


def test_path_anonymous(runner, tmp_path):
    global_result = runner.run("@@..path")
    local_result = runner.run("@..path", cwd=str(tmp_path))

    assert os.path.realpath(path_line(global_result)) == os.path.realpath(
        runner.byk_dir / "alias" / ".byk.json"
    )
    assert os.path.realpath(path_line(local_result)) == os.path.realpath(
        tmp_path / ".byk.json"
    )


def test_path_does_not_require_file(runner):
    assert not (runner.byk_dir / "alias" / "missing.byk.json").exists()

    result = runner.run("@@missing..path")

    path_line(result)


def test_path_rejects_arguments(runner):
    result = runner.run("@@ops..path", "extra")

    assert result.returncode == 1
    assert "path takes no arguments" in result.stderr


def test_path_is_completed_with_description(runner):
    result = runner.run("__api__", "complete", "--with-descriptions", "@@ops..p")

    assert result.returncode == 0
    assert result.stdout.splitlines() == ["@@ops..path\tPrint the absolute path"]


def test_resolve_rejects_path(runner):
    result = runner.run("__api__", "resolve", "@@ops..path")

    assert result.returncode == 1
    assert "file command" in result.stderr
