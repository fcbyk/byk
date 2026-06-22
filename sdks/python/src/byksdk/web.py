"""Web 应用辅助工具"""

from __future__ import annotations

import logging
from pathlib import Path
from typing import Any

from ._internal import requires

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