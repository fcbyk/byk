"""`@@@shell` 系统文件：在当前 shell 的启动文件里维护一段补全块。

这里验证的是四件容易出错的事：
- 只增删标记块，文件其余字节原样保留（含 CRLF）；
- 认 `install.sh` 写过的无标记裸行，老用户能删掉；
- 符号链接写的是目标，链接本身不能变成普通文件；
- 目标文件按 `$SHELL` / `$ZDOTDIR` / `$XDG_CONFIG_HOME` 推导，不按操作系统猜。
"""

import os

import pytest

BASH_PROFILE = "export PATH=/usr/local/bin:$PATH\n"

BLOCK = """# The following lines have been added by byk to enable byk cli completions.
if command -v byk >/dev/null 2>&1; then source <(byk __api__ completion {shell}); fi
# End of byk cli completions
"""

LEGACY = (
    "if command -v byk >/dev/null 2>&1; "
    "then source <(byk __api__ completion zsh); fi\n"
)


def shell_cmd(runner, *args, env=None, shell="/bin/zsh"):
    """跑一条 `@@@shell` 命令，环境固定当前 shell 为 zsh。"""
    return runner.run("@@@shell", *args, env={"SHELL": shell, "ZDOTDIR": "", **(env or {})})


def read(path):
    # 不能用 read_text：它默认做换行归一化，CRLF 会被洗成 LF，断言就假了
    return path.read_bytes().decode("utf-8")


def test_status_points_at_the_startup_file(runner):
    result = shell_cmd(runner)

    assert result.returncode == 0
    # 三个标签同一级，冒号后一个空格就是内容
    assert f"FilePath: {runner.home / '.zshrc'}" in result.stdout
    assert "Shell: zsh (from $SHELL)" in result.stdout
    assert "Completion: not installed" in result.stdout
    # 没装的时候不用另起一句提示：同屏的 Commands 就是「怎么装」
    assert "  add     Enable shell completion" in result.stdout


def test_bare_call_is_the_help_page(runner):
    # 不带参数不该另开一屏：状态与用法同屏，两个入口必须是同一份输出
    bare = shell_cmd(runner)
    help_page = shell_cmd(runner, "-h")

    assert bare.returncode == 0
    assert bare.stdout == help_page.stdout
    # 状态在前，用法定在后
    assert bare.stdout.index("Completion:") < bare.stdout.index("Usage:")
    assert "Usage: byk @@@shell [OPTIONS] [COMMAND] [ITEM]" in bare.stdout
    assert "  completion  Shell completion" in bare.stdout


def test_the_block_reads_like_prose(runner):
    rc = runner.home / ".zshrc"

    assert shell_cmd(runner, "add", "completion").returncode == 0

    # 写在别人的 rc 里，得看出是谁加的、到哪儿结束
    assert read(rc) == BLOCK.format(shell="zsh")


def test_add_writes_a_marked_block(runner):
    rc = runner.home / ".zshrc"
    rc.write_text("export FOO=1\n", encoding="utf-8")

    result = shell_cmd(runner, "add", "completion")

    assert result.returncode == 0
    assert f"Enabled zsh completion in {rc}" in result.stderr
    assert read(rc) == "export FOO=1\n\n" + BLOCK.format(shell="zsh")


def test_add_creates_a_missing_startup_file(runner):
    rc = runner.home / ".zshrc"

    result = shell_cmd(runner, "add", "completion")

    assert result.returncode == 0
    assert f"Created {rc}" in result.stderr
    assert read(rc) == BLOCK.format(shell="zsh")


def test_item_accepts_a_unique_prefix(runner):
    rc = runner.home / ".zshrc"

    # `comp` / `c` 都能落到 completion：命令已经够长，不该强迫敲全
    result = shell_cmd(runner, "add", "comp")

    assert result.returncode == 0
    assert f"Enabled zsh completion in {rc}" in result.stderr
    assert read(rc) == BLOCK.format(shell="zsh")

    assert shell_cmd(runner, "remove", "c").returncode == 0
    assert "# End of byk cli completions" not in read(rc)


def test_add_is_idempotent(runner):
    rc = runner.home / ".zshrc"

    assert shell_cmd(runner, "add", "completion").returncode == 0
    first = read(rc)
    again = shell_cmd(runner, "add", "completion")

    assert again.returncode == 0
    assert "already enabled" in again.stderr
    assert read(rc) == first
    assert first.count("# End of byk cli completions") == 1


def test_status_reports_the_block_lines(runner):
    rc = runner.home / ".zshrc"
    rc.write_text("export FOO=1\n", encoding="utf-8")
    shell_cmd(runner, "add", "completion")

    result = shell_cmd(runner)

    # export 在第 1 行，空行第 2 行，块从第 3 行开始
    assert "installed (lines 3-5)" in result.stdout
    assert "Hint:" not in result.stdout


def test_remove_restores_the_original_file(runner):
    rc = runner.home / ".zshrc"
    original = "export FOO=1\nexport BAR=2\n"
    rc.write_text(original, encoding="utf-8")
    shell_cmd(runner, "add", "completion")

    result = shell_cmd(runner, "remove", "completion")

    assert result.returncode == 0
    assert f"Disabled zsh completion in {rc}" in result.stderr
    assert read(rc) == original


def test_remove_takes_the_install_script_line(runner):
    rc = runner.home / ".zshrc"
    rc.write_text("export FOO=1\n" + LEGACY, encoding="utf-8")

    status = shell_cmd(runner)
    assert "installed (line 2, no markers)" in status.stdout

    result = shell_cmd(runner, "remove", "completion")

    assert result.returncode == 0
    assert read(rc) == "export FOO=1\n"


def test_add_on_top_of_the_install_script_line_only_hints(runner):
    rc = runner.home / ".zshrc"
    rc.write_text(LEGACY, encoding="utf-8")

    result = shell_cmd(runner, "add", "completion")

    assert result.returncode == 0
    assert "already enabled" in result.stderr
    assert "upgrades it to a marked block" in result.stderr
    # 不动它：重复的补全行比旧格式更糟
    assert read(rc) == LEGACY


def test_remove_without_a_block_is_a_no_op(runner):
    rc = runner.home / ".zshrc"
    rc.write_text("export FOO=1\n", encoding="utf-8")

    result = shell_cmd(runner, "remove", "completion")

    assert result.returncode == 0
    assert "not enabled" in result.stderr
    assert read(rc) == "export FOO=1\n"


def test_crlf_is_preserved(runner):
    rc = runner.home / ".zshrc"
    rc.write_bytes(b"export FOO=1\r\n")

    assert shell_cmd(runner, "add", "completion").returncode == 0

    assert read(rc) == "export FOO=1\r\n\r\n" + BLOCK.format(shell="zsh").replace(
        "\n", "\r\n"
    )


@pytest.mark.skipif(os.name == "nt", reason="需要符号链接")
def test_write_goes_through_a_symlink(runner):
    # dotfiles 仓库：家目录里的 .zshrc 是链接
    dotfiles = runner.home / "dotfiles"
    dotfiles.mkdir()
    real = dotfiles / "zshrc"
    real.write_text("export FOO=1\n", encoding="utf-8")
    link = runner.home / ".zshrc"
    os.symlink(real, link)

    # macOS 的临时目录在 /var 下，canonicalize 会解到 /private/var
    status = shell_cmd(runner)
    assert f"Target: {os.path.realpath(real)}" in status.stdout

    assert shell_cmd(runner, "add", "completion").returncode == 0

    assert link.is_symlink()
    assert read(real) == "export FOO=1\n\n" + BLOCK.format(shell="zsh")


def test_zdotdir_moves_the_startup_file(runner):
    zdot = runner.home / "zdot"
    zdot.mkdir()

    result = shell_cmd(runner, "add", "completion", env={"ZDOTDIR": str(zdot)})

    assert result.returncode == 0
    assert read(zdot / ".zshrc") == BLOCK.format(shell="zsh")
    assert not (runner.home / ".zshrc").exists()


def test_bash_follows_the_login_file(runner):
    profile = runner.home / ".profile"
    profile.write_text(BASH_PROFILE, encoding="utf-8")

    result = shell_cmd(runner, "add", "completion", shell="/bin/bash")

    # 有 .profile 就不能凭空建 .bash_profile 把它屏蔽掉
    assert result.returncode == 0
    assert f"Enabled bash completion in {profile}" in result.stderr
    assert read(profile) == BASH_PROFILE + "\n" + BLOCK.format(shell="bash")
    assert not (runner.home / ".bash_profile").exists()


def test_bash_creates_bash_profile_when_nothing_exists(runner):
    result = shell_cmd(runner, "add", "completion", shell="/bin/bash")

    assert result.returncode == 0
    assert read(runner.home / ".bash_profile") == BLOCK.format(shell="bash")


def test_fish_follows_xdg_config_home(runner):
    xdg = runner.home / "xdg"

    result = shell_cmd(
        runner,
        "add",
        "completion",
        env={"XDG_CONFIG_HOME": str(xdg)},
        shell="/usr/local/bin/fish",
    )

    assert result.returncode == 0
    assert read(xdg / "fish" / "config.fish") == BLOCK.format(shell="fish")


def test_rejects_unknown_commands_and_items(runner):
    unknown = shell_cmd(runner, "switch")
    assert unknown.returncode == 1
    assert unknown.stderr.strip() == 'Unknown command: "switch"\nAvailable: add, remove'

    item = shell_cmd(runner, "add", "path")
    assert item.returncode == 1
    assert item.stderr.strip() == 'Unknown item: "path"\nAvailable: completion'


def test_a_shell_argument_is_rejected_with_where_to_run_it(runner):
    result = shell_cmd(runner, "add", "completion", "bash")

    assert result.returncode == 1
    assert result.stderr.strip() == (
        "`add` takes no shell argument: it always updates the startup file of "
        "the shell running byk\n"
        "Hint: run `byk @@@shell add completion` from bash to set up bash"
    )
    # 一句都不落盘：参数这一层就挡住了
    assert not (runner.home / ".bash_profile").exists()
    assert not (runner.home / ".zshrc").exists()


def test_no_startup_file_is_a_normal_answer(runner):
    # 父进程是 pytest 的 python，认不出就交给 $SHELL；$SHELL 写着 sh 是有意义的
    status = shell_cmd(runner, shell="/bin/sh")

    assert status.returncode == 0
    assert "FilePath: (none)" in status.stdout
    assert "Shell: sh (from $SHELL)" in status.stdout
    assert "Completion: not available" in status.stdout
    assert "Only zsh, bash and fish have a startup file byk can edit" in status.stdout
    # 没有文件可改，`add` / `remove` 都用不上：不接用法清单
    assert "Usage:" not in status.stdout
    # 上下各留一个空行，和 zsh 那屏一样
    assert status.stdout.startswith("\n")
    assert status.stdout.endswith("file byk can edit\n\n")
    assert "Commands:" not in status.stdout
    assert not (runner.home / ".zshrc").exists()


def test_help_without_a_startup_file_is_just_the_state(runner):
    # 显式 `-h` 也是同一屏：这里没有能用的命令可列
    bare = shell_cmd(runner, shell="/bin/sh")
    help_page = shell_cmd(runner, "-h", shell="/bin/sh")

    assert bare.stdout == help_page.stdout
    assert "Usage:" not in help_page.stdout


def test_modifying_without_a_startup_file_fails(runner):
    result = shell_cmd(runner, "add", "completion", shell="/bin/sh")

    assert result.returncode == 1
    assert result.stderr.strip() == (
        "No startup file for sh (from $SHELL)\n"
        "Only zsh, bash and fish have a startup file byk can edit"
    )
    assert not (runner.home / ".zshrc").exists()


def test_no_shell_argument_completes_to_nothing(runner):
    def complete(*words):
        return runner.run("__api__", "complete", *words).stdout.split()

    assert complete("@@@shell", "") == ["add", "remove"]
    assert complete("@@@shell", "a") == ["add"]
    assert complete("@@@shell", "add", "") == ["completion"]
    # 目标 shell 由运行环境决定，补个 bash 出来等于教用户去做一件会被拒绝的事
    assert complete("@@@shell", "add", "completion", "") == []
