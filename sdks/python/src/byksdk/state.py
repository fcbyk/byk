"""基于 JSON 文件的状态存储"""

from __future__ import annotations

import json
from dataclasses import dataclass
from pathlib import Path
import tempfile
from typing import Any


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

        from byksdk.paths import STATE_DIR
        from byksdk.state import StateStore

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