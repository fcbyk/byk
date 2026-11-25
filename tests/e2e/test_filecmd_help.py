def test_file_home_matches_main_help_layout(runner):
    bare = runner.run("global@ops")
    long_flag = runner.run("global@ops", "--help")
    short_flag = runner.run("global@ops", "-h")

    assert bare.returncode == 0
    assert bare.stderr == ""
    assert bare.stdout == long_flag.stdout == short_flag.stdout
    assert bare.stdout.startswith("\nCommands for this global alias file. Run `byk global@ops <command> -h` for details.\n\nFilePath: ")
    assert bare.stdout.endswith("\n\n")

    lines = bare.stdout.splitlines()
    assert lines[0] == ""
    assert lines[1] == (
        "Commands for this global alias file. "
        "Run `byk global@ops <command> -h` for details."
    )
    assert lines[2] == ""
    assert lines[3].startswith("FilePath: ")
    assert lines[3].endswith("ops.byk.json (not created yet)")
    assert lines[4] == "Usage: byk global@ops [OPTIONS] [COMMAND] [ARGS]..."
    assert lines[5] == ""
    assert lines[6] == "Options:"
    assert lines[7] == "  --help, -h  Show this help"
    assert lines[8] == ""
    assert lines[9] == "Commands:"
    assert "  init     Create this alias file" in lines
    assert "  destroy  Delete this alias file" in lines
    assert "  clear    Reset this alias file to an empty object" in lines
    assert "  set      Create or update an alias" in lines
    assert "  remove   Remove an alias or group" in lines
    assert "  json     Show this alias file's JSON" in lines
    assert "  path     Print this alias file's absolute path" in lines
    assert "  rename   Rename this alias file" in lines
    assert "  clone    Copy a local or remote alias file into this file" in lines
    assert "  link     Point this file at another alias file" in lines
    assert "  deps     Install, uninstall, or inspect this file's dependencies" in lines
    assert not any(line.startswith("  help ") for line in lines)


def test_help_for_command_shows_usage_and_options(runner):
    short_flag = runner.run("global@ops", "set", "-h")
    long_flag = runner.run("global@ops", "set", "--help")

    assert short_flag.returncode == 0
    assert short_flag.stderr == ""
    assert short_flag.stdout == long_flag.stdout
    assert short_flag.stdout.startswith(
        "\nCreate or update an alias\n\nFilePath: "
    )
    assert short_flag.stdout.endswith("\n\n")

    lines = short_flag.stdout.splitlines()
    assert lines[1] == "Create or update an alias"
    assert lines[3].startswith("FilePath: ")
    assert lines[3].endswith("ops.byk.json (not created yet)")
    assert (
        "Usage: byk global@ops set [OPTIONS] [key]" in short_flag.stdout
    )
    assert "Options:" in lines
    assert any(line.startswith("  --help, -h") and "Show this help" in line for line in lines)
    assert "  --cmd <command>       Set the alias command" in lines
    assert "  --cwd <path>          Set the working directory" in lines
    assert "  --mode <mode>         Set the execution mode" in lines
    assert "  --description <text>  Set the description" in lines
    assert "  --recursive           Allow re-entry on the call chain" in lines
    assert "  --no-recursive        Remove the recursive flag" in lines
    assert (
        "  --private             Keep this file or alias out of merged lookup" in lines
    )
    assert "  --no-private          Expose it to merged lookup again" in lines
    assert "  --unset <fields>      Remove metadata fields" in lines
    assert "  --priority <n>        Set the file priority (omit key)" in lines
    assert (
        "  --paths <dir>         Prepend directories to PATH (omit key)" in lines
    )
    assert not any("global@ops init" in line for line in lines)


def test_help_flag_after_arguments_targets_the_verb(runner):
    expected = runner.run("global@ops", "set", "-h")
    result = runner.run("global@ops", "set", "build", "-h")

    assert result.returncode == 0
    assert result.stdout == expected.stdout


def test_help_for_command_without_options(runner):
    result = runner.run("global@ops", "path", "-h")

    assert result.returncode == 0
    assert result.stdout.startswith(
        "\nPrint this alias file's absolute path\n\nFilePath: "
    )
    assert "Usage: byk global@ops path [OPTIONS]" in result.stdout
    assert "  --help, -h  Show this help" in result.stdout


def test_help_local_and_anonymous_prefixes(runner):
    local = runner.run("local@ops", "-h")
    anonymous = runner.run("global@", "-h")

    assert local.returncode == 0
    assert local.stdout.startswith(
        "\nCommands for this local alias file. "
        "Run `byk local@ops <command> -h` for details.\n\nFilePath: "
    )
    assert "Usage: byk local@ops [OPTIONS] [COMMAND] [ARGS]..." in local.stdout
    assert any(line.startswith("  init ") for line in local.stdout.splitlines())
    assert anonymous.returncode == 0
    assert anonymous.stdout.startswith(
        "\nCommands for this global alias file. "
        "Run `byk global@ <command> -h` for details.\n\nFilePath: "
    )
    assert "Usage: byk global@ [OPTIONS] [COMMAND] [ARGS]..." in anonymous.stdout
    assert any(line.startswith("  init ") for line in anonymous.stdout.splitlines())

    local_set = runner.run("local@ops", "set", "-h")
    anonymous_set = runner.run("global@", "set", "-h")
    assert local_set.returncode == 0
    assert "Usage: byk local@ops set [OPTIONS] [key]" in local_set.stdout
    assert anonymous_set.returncode == 0
    assert "Usage: byk global@ set [OPTIONS] [key]" in anonymous_set.stdout


def test_help_is_not_a_command(runner):
    result = runner.run("global@ops", "help")

    assert result.returncode == 1
    assert 'Unknown file command: "help"' in result.stderr
    assert (
        "Available: init, destroy, clear, set, remove, json, path, rename, clone, link, deps"
        in result.stderr
    )
    assert "help" not in result.stderr.split("Available:", 1)[1].split("Hint:", 1)[0]


def test_unknown_command_lists_available_commands(runner):
    result = runner.run("global@ops", "bogus")

    assert result.returncode == 1
    assert 'Unknown file command: "bogus"' in result.stderr
    assert (
        "Available: init, destroy, clear, set, remove, json, path, rename, clone, link, deps"
        in result.stderr
    )


def test_help_flags_are_completed(runner):
    verbs = runner.run("__api__", "complete", "global@ops", "")
    short = runner.run("__api__", "complete", "global@ops", "-")
    options = runner.run("__api__", "complete", "global@ops", "set", "--")

    assert verbs.returncode == 0
    assert "help" not in verbs.stdout.splitlines()
    assert "init" in verbs.stdout.splitlines()
    assert short.returncode == 0
    assert short.stdout.splitlines() == ["--help", "-h"]
    assert options.returncode == 0
    assert "--help" in options.stdout.splitlines()


def test_resolve_rejects_help_flag(runner):
    result = runner.run("__api__", "resolve", "global@ops", "-h")

    assert result.returncode == 1
    assert "file command" in result.stderr
