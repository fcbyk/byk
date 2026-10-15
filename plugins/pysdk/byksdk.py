from __future__ import annotations

import json
from dataclasses import dataclass, field
from pathlib import Path
import tempfile
from typing import Any
import logging


# ---------------------
# 基于 JSON 文件的状态存储
# ---------------------
def _read_json(path: Path) -> dict[str, Any]:
    """读取 JSON 文件，不存在或损坏时返回空字典"""
    if not path.exists():
        return {}
    try:
        with path.open("r", encoding="utf-8") as f:
            data = json.load(f)
    except (json.JSONDecodeError, OSError):
        return {}
    return data if isinstance(data, dict) else {}


def _write_json(path: Path, data: Any) -> None:
    """原子写入 JSON 文件"""
    path.parent.mkdir(parents=True, exist_ok=True)
    with tempfile.NamedTemporaryFile(
        "w",
        delete=False,
        dir=path.parent,
        encoding="utf-8",
    ) as f:
        json.dump(data, f, indent=2, ensure_ascii=False)
        tmp = Path(f.name)
    tmp.replace(path)


@dataclass(slots=True)
class StateStore:
    """基于 JSON 文件的状态存储。

    用法::

        from byksdk import StateStore, STATE_DIR

        store = StateStore(STATE_DIR / "my_config.json")
        store.set("key", "value")
        print(store.get("key"))
    """

    path: Path

    def load(self) -> dict[str, Any]:
        """读取全部数据"""
        return _read_json(self.path)

    def save(self, data: dict[str, Any]) -> dict[str, Any]:
        """覆盖保存全部数据，返回保存后的数据"""
        _write_json(self.path, data)
        return data

    def get(self, key: str, default: Any = None) -> Any:
        """获取单个值"""
        return self.load().get(key, default)

    def set(self, key: str, value: Any) -> dict[str, Any]:
        """设置单个值，返回完整数据"""
        data = self.load()
        data[key] = value
        return self.save(data)

    def update(self, values: dict[str, Any]) -> dict[str, Any]:
        """批量更新，返回更新后的数据"""
        data = self.load()
        data.update(values)
        return self.save(data)

    def delete(self, key: str) -> dict[str, Any]:
        """删除单个值，返回剩余数据"""
        data = self.load()
        data.pop(key, None)
        return self.save(data)

    def clear(self) -> dict[str, Any]:
        """清空所有数据，返回空字典"""
        return self.save({})


# -------
# 路径布局
# -------
ROOT_DIR = Path.home() / ".byk"
STATE_DIR = ROOT_DIR / "state"
PLUGINS_DIR = ROOT_DIR / "plugins"
LOGS_DIR = ROOT_DIR / "logs"
RUNTIME_DIR = ROOT_DIR / "runtime"
CACHE_DIR = ROOT_DIR / "cache"
PLUGINS_LOG_FILE = LOGS_DIR / "plugins.log"


def ensure_dirs() -> None:
    """确保所有目录存在"""
    for directory in (ROOT_DIR, STATE_DIR, PLUGINS_DIR, LOGS_DIR, RUNTIME_DIR, CACHE_DIR):
        directory.mkdir(parents=True, exist_ok=True)


# -------
# 统一日志
# -------
def get_logger(name: str | None = None) -> logging.Logger:
    """获取日志实例，自动创建父目录

    Args:
        name: 插件名，None 时返回 SDK 全局日志

    Returns:
        logger 实例
    """
    if name is None:
        logger_name = "byk"
        log_file = PLUGINS_LOG_FILE
    else:
        logger_name = f"byk.{name}"
        log_file = PLUGINS_DIR / name / f"{name}.log"

    logger = logging.getLogger(logger_name)
    if logger.handlers:
        return logger

    logger.setLevel(logging.INFO)
    log_file.parent.mkdir(parents=True, exist_ok=True)
    handler = logging.FileHandler(log_file, encoding="utf-8")
    handler.setFormatter(
        logging.Formatter(
            fmt="%(asctime)s %(levelname)s [%(name)s] %(message)s",
            datefmt="%Y-%m-%d %H:%M:%S",
        )
    )
    logger.addHandler(handler)
    logger.propagate = False
    return logger


# --------
# 插件上下文
# --------
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