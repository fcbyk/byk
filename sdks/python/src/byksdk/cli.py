"""CLI 输出与辅助工具"""

from __future__ import annotations

import time
import urllib.error
import urllib.request
import webbrowser as _webbrowser
from typing import Any

from ._internal import requires
from .network import ensure_port_available


@requires("click", version="8.0.0")
def check_port(
    port: int,
    host: str = "0.0.0.0",
    output_prefix: str = " ",
    silent: bool = False,
) -> bool:
    """检查端口是否可用（需 click>=8.0.0）

    Args:
        port: 要检查的端口号
        host: 绑定地址，默认 0.0.0.0
        output_prefix: 错误输出前缀，用于对齐
        silent: True 时仅返回布尔值，不打印任何内容

    Returns:
        True 表示端口可用
    """
    import click

    try:
        ensure_port_available(port=port, host=host)
    except OSError as e:
        if not silent:
            click.echo(
                f"{output_prefix}Error: Port {port} is already in use "
                f"(or you don't have permission). "
                f"{output_prefix}Please choose another port "
                f"(e.g. --port {int(port) + 1})."
            )
            click.echo(f"{output_prefix}Details: {e}\n")
        return False
    return True


@requires("click", version="8.0.0")
def colored_key_value(
    key: str,
    value: Any,
    key_color: str | None = "cyan",
    value_color: str | None = "yellow",
) -> str:
    """生成带颜色的键值字符串（需 click>=8.0.0）

    Args:
        key: 键文本
        value: 值（自动转为字符串）
        key_color: 键的颜色名称（None 表示无颜色）
        value_color: 值的颜色名称（None 表示无颜色）

    Returns:
        拼接好的 ANSI 颜色字符串
    """
    import click

    return (
        f"{click.style(str(key), fg=key_color)}: "
        f"{click.style(str(value), fg=value_color)}"
    )


@requires("click", version="8.0.0")
def echo_network_urls(
    networks: list[dict[str, Any]],
    port: int,
    include_virtual: bool = False,
) -> None:
    """打印可访问的本地和局域网 URL（需 click>=8.0.0）

    Args:
        networks: get_private_networks() 的返回值
        port: 服务端口号
        include_virtual: 是否输出虚拟网卡（docker/vmware 等）的地址
    """
    import click

    for host in ("localhost", "127.0.0.1"):
        click.echo(
            colored_key_value(
                " Local", f"http://{host}:{port}",
                key_color=None, value_color="cyan",
            )
        )

    for net in networks:
        if net.get("virtual") and not include_virtual:
            continue
        for ip in net.get("ips", []):
            if ip == "127.0.0.1":
                continue
            iface = net.get("iface", "unknown")
            click.echo(
                colored_key_value(
                    f" [{iface}] Network URL:",
                    f"http://{ip}:{port}",
                    key_color=None,
                    value_color="cyan",
                )
            )


@requires("pyperclip", version="1.9.0")
def copy_to_clipboard(
    text: str,
    label: str = "URL",
    output_prefix: str = " ",
    silent: bool = False,
) -> None:
    """将文本复制到系统剪贴板（需 pyperclip>=1.9.0）

    Args:
        text: 要复制的文本
        label: 成功/失败提示中的标签名
        output_prefix: 输出行前缀，用于对齐
        silent: True 时静默执行，不输出任何提示
    """
    import click
    import pyperclip

    try:
        pyperclip.copy(text)
        if not silent:
            click.echo(f"{output_prefix}{label} has been copied to clipboard")
    except Exception:
        if not silent:
            click.echo(
                f"{output_prefix}Warning: Could not copy {label} to clipboard"
            )


def wait_for_server_ready(
    port: int,
    host: str = "127.0.0.1",
    timeout: float = 10.0,
) -> bool:
    """轮询直到 Web 服务可正常响应

    纯标准库实现，无额外依赖

    Args:
        port: 服务端口
        host: 服务地址，默认 127.0.0.1
        timeout: 最长等待秒数

    Returns:
        True 表示在超时前收到了 200 响应
    """
    url = f"http://{host}:{port}/"
    deadline = time.monotonic() + timeout
    while time.monotonic() < deadline:
        try:
            with urllib.request.urlopen(url, timeout=0.5) as resp:
                if resp.status == 200:
                    return True
        except (urllib.error.URLError, TimeoutError, OSError):
            pass
        time.sleep(0.05)
    return False


def open_browser(url: str) -> None:
    """在系统默认浏览器中打开 URL

    纯标准库实现，无额外依赖
    """
    _webbrowser.open(url)