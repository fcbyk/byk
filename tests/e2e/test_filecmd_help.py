def test_help_lists_commands_without_options(runner):
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
    assert "  @@ops..set      Create or update an alias" in lines
    assert "  @@ops..remove   Remove an alias or group" in lines
    assert "  @@ops..json     Show the alias file JSON" in lines
    assert "  @@ops..path     Print the absolute path" in lines
    assert "  @@ops..rename   Rename the alias file" in lines
    assert "  @@ops..link     Point this file at another alias file" in lines
    assert "  @@ops..help     Show this help" in lines
    assert "Options:" not in lines
    assert "Use @@ops..help <command> to see usage and options." in lines


def test_help_for_command_shows_usage_and_options(runner):
    result = runner.run("@@ops..help", "set")

    assert result.returncode == 0
    assert result.stderr == ""
    assert result.stdout.startswith("\n@@ops..set <key> [options]\n")
    assert result.stdout.endswith("\n\n")
    lines = result.stdout.splitlines()
    assert lines[1] == "@@ops..set <key> [options]"
    assert lines[2] == "Create or update an alias"
    assert "Options:" in lines
    assert "  --cmd <command>       Set the alias command" in lines
    assert "  --cwd <path>          Set the working directory" in lines
    assert "  --mode <mode>         Set the execution mode" in lines
    assert "  --description <text>  Set the description" in lines
    assert "  --recursive           Allow re-entry on the call chain" in lines
    assert "  --no-recursive        Remove the recursive flag" in lines
    assert "  --unset <fields>      Remove metadata fields" in lines
    assert not any("@@ops..init" in line for line in lines)


def test_help_for_command_without_options(runner):
    result = runner.run("@@ops..help", "path")

    assert result.returncode == 0
    assert result.stdout.startswith("\n@@ops..path\nPrint the absolute path\n")
    assert "Options:" not in result.stdout


def test_help_local_and_anonymous_prefixes(runner):
    local = runner.run("@ops..help")
    anonymous = runner.run("@@..help")

    assert local.returncode == 0
    assert any(line.startswith("  @ops..init") for line in local.stdout.splitlines())
    assert anonymous.returncode == 0
    assert any(line.startswith("  @@..init") for line in anonymous.stdout.splitlines())
    assert any(line.startswith("  @@..help") for line in anonymous.stdout.splitlines())

    local_set = runner.run("@ops..help", "set")
    anonymous_set = runner.run("@@..help", "set")
    assert local_set.returncode == 0
    assert "@ops..set <key> [options]" in local_set.stdout
    assert anonymous_set.returncode == 0
    assert "@@..set <key> [options]" in anonymous_set.stdout


def test_help_rejects_extra_arguments(runner):
    result = runner.run("@@ops..help", "set", "extra")

    assert result.returncode == 1
    assert "help takes at most one command" in result.stderr


def test_help_rejects_unknown_command(runner):
    result = runner.run("@@ops..help", "bogus")

    assert result.returncode == 1
    assert 'Unknown file command: "bogus"' in result.stderr
    assert (
        "Available: init, destroy, clear, set, remove, json, path, rename, link, help"
        in result.stderr
    )


def test_help_is_completed_with_description(runner):
    result = runner.run("__api__", "complete", "--with-descriptions", "@@ops..")

    assert result.returncode == 0
    assert "@@ops..help\tShow this help" in result.stdout.splitlines()


def test_help_argument_completes_command_names(runner):
    result = runner.run("__api__", "complete", "@@ops..help", "")

    assert result.returncode == 0
    assert result.stdout.splitlines() == [
        "init",
        "destroy",
        "clear",
        "set",
        "remove",
        "json",
        "path",
        "rename",
        "link",
    ]


def test_help_argument_completion_excludes_help(runner):
    result = runner.run("__api__", "complete", "@@ops..help", "h")

    assert result.returncode == 0
    assert result.stdout == ""


def test_help_argument_completion_stops_after_command(runner):
    result = runner.run("__api__", "complete", "@@ops..help", "set", "")

    assert result.returncode == 0
    assert result.stdout == ""


def test_resolve_rejects_help(runner):
    result = runner.run("__api__", "resolve", "@@ops..help")

    assert result.returncode == 1
    assert "file command" in result.stderr
