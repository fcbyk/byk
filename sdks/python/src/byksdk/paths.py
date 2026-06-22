"""路径布局"""

from pathlib import Path


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