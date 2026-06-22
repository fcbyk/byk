"""插件上下文"""

from __future__ import annotations

import logging
from dataclasses import dataclass, field
from pathlib import Path

from .logging import get_logger
from .paths import PLUGINS_DIR, STATE_DIR
from .state import StateStore


@dataclass
class _AppContext:
    """应用上下文，跨插件共享"""

    logger: logging.Logger = field(default_factory=get_logger)

    def store(self, name: str = "plugins.state") -> StateStore:
        """获取全局持久化实例

        Args:
            name: 存储名称，默认 "plugins.state"

        Returns:
            StateStore，路径为 state/{name}.json
        """
        return StateStore(STATE_DIR / f"{name}.json")


@dataclass
class PluginContext:
    """插件上下文，统一管理插件专属的持久化和日志

    用法::

        from byksdk import plugin

        ctx = plugin("server")
        ctx.state().set("port", 8080)
        ctx.state("config").set("timeout", 30)
        ctx.app.store().set("theme", "dark")
        ctx.app.logger.info("app started")
        ctx.logger.info("server started")
        ctx.home  # plugins/server/
    """

    name: str
    home: Path
    logger: logging.Logger
    app: _AppContext = field(default_factory=_AppContext)

    def state(self, name: str = "state") -> StateStore:
        """获取插件专属持久化实例

        Args:
            name: 存储名称，默认 "state"

        Returns:
            StateStore，路径为 plugins/{name}/{name}.json
        """
        return StateStore(self.home / f"{name}.json")


def plugin(name: str) -> PluginContext:
    """获取插件上下文

    Args:
        name: 插件名

    Returns:
        PluginContext，home 为 plugins/{name}/，
        state() 默认路径为 plugins/{name}/state.json，
        app.store() 默认路径为 state/plugins.state.json，
        logger 写入 plugins/{name}/{name}.log
    """
    home = PLUGINS_DIR / name
    return PluginContext(
        name=name,
        home=home,
        logger=get_logger(name),
    )