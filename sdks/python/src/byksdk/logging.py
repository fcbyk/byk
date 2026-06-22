"""统一日志"""

from __future__ import annotations

import logging

from .paths import PLUGINS_DIR, PLUGINS_LOG_FILE


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