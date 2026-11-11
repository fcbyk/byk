def test_help_lists_commands_and_options_without_file(runner):
    result = runner.run("@@ops..help")

    assert result.returncode == 0
    assert result.stderr == ""
    assert result.stdout.startswith("\n")
    assert result.stdout.endswith("\n\n")
    lines = result.stdout.splitlines()
    assert lines[0] == ""
    assert lines[1] == "File commands:"
    assert "  @@ops..init     Create an alias file" in lines
    assert "  @@ops..destroy  Remove the alias file" in lines
    assert "  @@ops..clear    Reset this file to an empty object" in lines
    assert "  @@ops..json     Show the alias file JSON" in lines
    assert "  @@ops..path     Print the absolute path" in lines
    assert "  @@ops..rename   Rename the alias file" in lines
    assert "  @@ops..link     Point this file at another alias file" in lines
    assert "  @@ops..help     Show this help" in lines
    assert "Options:" in lines
    assert "  @@ops..init --link      Create as a link to another alias file" in lines
    assert "  @@ops..destroy --yes    Skip confirmation" in lines
    assert "  @@ops..clear --yes      Skip confirmation" in lines
    assert (
        "  @@ops..json --expand    Expand file variables and pretty-print" in lines
    )
    assert "  @@ops..json --no-check  Skip JSON validation and print raw" in lines


def test_help_local_and_anonymous_prefixes(runner):
    local = runner.run("@ops..help")
    anonymous = runner.run("@@..help")

    assert local.returncode == 0
    assert any(line.startswith("  @ops..init") for line in local.stdout.splitlines())
    assert anonymous.returncode == 0
    assert any(line.startswith("  @@..init") for line in anonymous.stdout.splitlines())
    assert any(line.startswith("  @@..help") for line in anonymous.stdout.splitlines())


def test_help_rejects_arguments(runner):
    result = runner.run("@@ops..help", "extra")

    assert result.returncode == 1
    assert "help takes no arguments" in result.stderr


def test_help_is_completed_with_description(runner):
    result = runner.run("__api__", "complete", "--with-descriptions", "@@ops..")

    assert result.returncode == 0
    assert "@@ops..help\tShow this help" in result.stdout.splitlines()


def test_resolve_rejects_help(runner):
    result = runner.run("__api__", "resolve", "@@ops..help")

    assert result.returncode == 1
    assert "file command" in result.stderr
