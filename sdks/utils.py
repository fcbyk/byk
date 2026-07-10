from __future__ import annotations

import functools, socket, logging, time
from importlib.util import find_spec
from typing import List, Dict, Any
from pathlib import Path
import urllib.error, urllib.request
import webbrowser as _webbrowser


# --------------
# 依赖检查工具函数
# --------------
def require_dependency(
    package: str,
    scope: str,
    version: str | None = None,
    hint: str | None = None,
) -> None:
    """校验第三方依赖是否可用，不可用时抛出带安装指引的 ImportError

    Args:
        package: pip 包名，如 "flask"、"click"
        scope: 使用说明，如 "byksdk.web"
        version: 最低版本号，如 "2.0"，会自动生成 >= 约束
        hint: 完全自定义安装命令，传入时忽略 version
    """
    if find_spec(package) is None:
        spec = f"{package}>={version}" if version else package
        install = hint or f"pip install {spec}"
        raise ImportError(
            f"{package} is required for {scope}.\n"
            f"Install it and add to your plugin dependencies:\n"
            f"    {install}\n"
            f"    # then add '{spec}' to pyproject.toml dependencies"
        )


def requires(package: str, version: str | None = None, hint: str | None = None):
    """依赖检查装饰器，调用函数前自动校验第三方包是否可用

    可叠加使用以声明多个依赖:

        @requires("pyperclip", version="1.9.0")
        @requires("click", version="8.0.0")
        def copy_to_clipboard(text): ...

    Args:
        package: pip 包名
        version: 最低版本号，自动生成 >= 约束
        hint: 完全自定义安装命令
    """
    def decorator(func):
        @functools.wraps(func)
        def wrapper(*args, **kwargs):
            scope = f"{func.__module__}.{func.__qualname__}"
            require_dependency(package, scope, version=version, hint=hint)
            return func(*args, **kwargs)
        return wrapper
    return decorator


# -----------
# 网络工具函数
# -----------

# 网络接口分类规则：(关键词列表，类型名称，是否虚拟接口，优先级)
IFACE_RULES = [
    (['vmware', 'vmnet'],         'vmware',     True,  30),
    (['vbox', 'virtualbox'],      'virtualbox', True,  30),
    (['docker', 'wsl'],           'container',  True,  40),
    (['bluetooth'],               'bluetooth',  True,  60),
    (['ethernet', '以太网'],       'ethernet',   False, 10),
    (['wlan', 'wi-fi', '无线'],   'wifi',       False, 10),
    (['loopback'],                'loopback',   True,  100),
]


def detect_iface_type(iface: str) -> tuple[str, bool, int]:
    """检测网络接口类型"""
    lname = iface.lower()
    for keywords, t, virtual, prio in IFACE_RULES:
        if any(k in lname for k in keywords):
            return t, virtual, prio
    return 'unknown', False, 50


@requires("psutil", version="5.9.0")
def get_private_networks() -> List[Dict[str, Any]]:
    """获取所有局域网 IP 地址信息（需 psutil>=5.9.0）"""
    import psutil

    results = []
    interfaces = psutil.net_if_addrs()

    for iface, addrs in interfaces.items():
        ips = []
        iface_type, is_virtual, priority = detect_iface_type(iface)

        for addr in addrs:
            if addr.family != socket.AF_INET:
                continue

            ip = addr.address

            # 跳过回环地址和链路本地地址
            if ip.startswith('127.'):
                continue
            if ip.startswith('169.254.'):
                continue
            # 只保留私有 IP 地址 (RFC 1918)
            if ip.startswith('10.') or ip.startswith('192.168.'):
                ips.append(ip)
            elif ip.startswith('172.'):
                second_octet = int(ip.split('.')[1])
                if 16 <= second_octet <= 31:
                    ips.append(ip)

        if ips:
            results.append({
                "iface": iface,
                "ips": ips,
                "type": iface_type,
                "virtual": is_virtual,
                "priority": priority
            })

    # 按优先级排序（数字越小优先级越高）
    results.sort(key=lambda x: x['priority'])
    
    # 如果没有找到任何局域网 IP，返回 localhost
    if not results:
        results.append({
            "iface": "localhost",
            "ips": ["127.0.0.1"],
            "type": "loopback",
            "virtual": True,
            "priority": 100
        })
    
    return results


def ensure_port_available(port: int, host: str = "0.0.0.0") -> None:
    """检查端口是否可用，不可用时抛出 PortBusyError

    Raises:
        PortBusyError: 端口已被占用
    """
    try:
        with socket.socket(socket.AF_INET, socket.SOCK_STREAM) as s:
            s.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
            s.bind((host, int(port)))
    except OSError as e:
        raise PortBusyError(port, host) from e


class PortBusyError(OSError):
    """端口被占用异常"""

    def __init__(self, port: int, host: str = "0.0.0.0") -> None:
        self.port = port
        self.host = host
        super().__init__(f"Port {port} on {host} is already in use")



# --------------
# Web 应用辅助工具
# --------------
log = logging.getLogger("werkzeug")
log.setLevel(logging.ERROR)


@requires("flask", version="2.0")
def create_spa(
    static_dir: str | Path,
    entry_html: str = "index.html",
    page: list[str] | None = None,
    cli_data: Any = None,
):
    """创建托管单页应用 (SPA) 的 Flask 实例（需 flask>=2.0）

    自动映射 /assets/* 到静态资源目录，并将所有未知路由回退到
    entry_html（支持前端路由）。响应添加无缓存头

    Args:
        static_dir: 打包后的 SPA 根目录（含 index.html 和 assets/）
        entry_html: SPA 入口文件名，默认 index.html
        page: 多页面路由列表，如 ["/admin", "/login"]
        cli_data: 可选，挂载到 app.cli_data 供后续使用

    Returns:
        Flask 应用实例
    """
    from flask import Flask, make_response, send_from_directory

    static_dir = Path(static_dir).resolve()
    assets_dir = static_dir / "assets"

    app = Flask(
        __name__,
        static_folder=str(assets_dir),
        static_url_path="/assets",
    )

    @app.route("/")
    def index():
        response = make_response(
            send_from_directory(str(static_dir), entry_html)
        )
        response.headers["Cache-Control"] = (
            "no-cache, no-store, must-revalidate"
        )
        response.headers["Pragma"] = "no-cache"
        response.headers["Expires"] = "0"
        return response

    if page:
        for url in page:

            def view(
                _entry_html: str = entry_html,
                _static_dir: str = str(static_dir),
            ):
                response = make_response(
                    send_from_directory(_static_dir, _entry_html)
                )
                response.headers["Cache-Control"] = (
                    "no-cache, no-store, must-revalidate"
                )
                response.headers["Pragma"] = "no-cache"
                response.headers["Expires"] = "0"
                return response

            endpoint = f"page_{url.strip('/').replace('/', '_') or 'root'}"
            app.add_url_rule(url, endpoint, view)

    if cli_data is not None:
        app.cli_data = cli_data

    return app


class R:
    """Web API 统一响应格式"""

    @staticmethod
    @requires("flask", version="2.0")
    def success(data: Any = None, message: str = "success"):
        """构造成功响应（需 flask>=2.0）

        Returns:
            (Flask Response, 200)
        """
        from flask import jsonify

        return jsonify({
            "code": 200,
            "message": message,
            "data": data,
        }), 200

    @staticmethod
    @requires("flask", version="2.0")
    def error(message: str = "error", code: int = 400, data: Any = None):
        """构造错误响应（需 flask>=2.0）

        Returns:
            (Flask Response, <code>)
        """
        from flask import jsonify

        return jsonify({
            "code": code,
            "message": message,
            "data": data,
        }), code


@requires("flask", version="2.0")
def get_client_ip() -> str:
    """提取客户端 IP（需 flask>=2.0）

    优先使用 X-Forwarded-For 头（取第一个），
    回退到 request.remote_addr

    Returns:
        客户端 IP 字符串，无法获取时返回 "unknown"
    """
    from flask import request

    xff = request.headers.get("X-Forwarded-For", "")
    if xff:
        return xff.split(",")[0].strip()
    return request.remote_addr or "unknown"


# ----------------
# CLI 输出与辅助工具
# ----------------
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
@requires("click", version="8.0.0")
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