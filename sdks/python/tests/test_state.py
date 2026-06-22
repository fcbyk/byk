"""测试 state.py 模块"""

import json
from pathlib import Path

import pytest

from byksdk.state import StateStore


class TestStateStore:
    """测试 StateStore"""

    def test_load_returns_empty_for_nonexistent_file(self, tmp_path):
        store = StateStore(tmp_path / "nonexistent.json")
        assert store.load() == {}

    def test_load_returns_empty_for_corrupted_file(self, tmp_path):
        path = tmp_path / "corrupted.json"
        path.write_text("not valid json")
        store = StateStore(path)
        assert store.load() == {}

    def test_load_returns_empty_for_non_dict_json(self, tmp_path):
        path = tmp_path / "list.json"
        path.write_text('[1, 2, 3]')
        store = StateStore(path)
        assert store.load() == {}

    def test_set_and_get(self, tmp_path):
        store = StateStore(tmp_path / "data.json")
        result = store.set("key", "value")
        assert result == {"key": "value"}
        assert store.get("key") == "value"

    def test_get_with_default(self, tmp_path):
        store = StateStore(tmp_path / "data.json")
        assert store.get("missing", "default") == "default"
        assert store.get("missing") is None

    def test_save_overwrites(self, tmp_path):
        store = StateStore(tmp_path / "data.json")
        store.set("a", 1)
        store.save({"x": "y"})
        assert store.load() == {"x": "y"}
        assert store.get("a") is None

    def test_update(self, tmp_path):
        store = StateStore(tmp_path / "data.json")
        store.set("keep", "me")
        result = store.update({"a": 1, "b": 2})
        assert result == {"keep": "me", "a": 1, "b": 2}

    def test_delete(self, tmp_path):
        store = StateStore(tmp_path / "data.json")
        store.update({"a": 1, "b": 2})
        result = store.delete("a")
        assert result == {"b": 2}
        assert store.get("a") is None

    def test_delete_nonexistent_key(self, tmp_path):
        store = StateStore(tmp_path / "data.json")
        store.set("a", 1)
        result = store.delete("missing")
        assert result == {"a": 1}

    def test_clear(self, tmp_path):
        store = StateStore(tmp_path / "data.json")
        store.update({"a": 1, "b": 2})
        result = store.clear()
        assert result == {}
        assert store.load() == {}

    def test_persistence_across_instances(self, tmp_path):
        path = tmp_path / "shared.json"
        store1 = StateStore(path)
        store1.set("key", "value")

        store2 = StateStore(path)
        assert store2.get("key") == "value"

    def test_creates_parent_directories(self, tmp_path):
        store = StateStore(tmp_path / "deep" / "nested" / "data.json")
        store.set("key", "value")
        assert (tmp_path / "deep" / "nested" / "data.json").exists()

    def test_atomic_write_does_not_corrupt(self, tmp_path):
        path = tmp_path / "data.json"
        path.write_text('{"old": "data"}')
        store = StateStore(path)
        store.set("key", "value")
        data = json.loads(path.read_text())
        assert data == {"old": "data", "key": "value"}

    def test_set_returns_full_data(self, tmp_path):
        store = StateStore(tmp_path / "data.json")
        store.set("a", 1)
        result = store.set("b", 2)
        assert result == {"a": 1, "b": 2}

    def test_update_returns_full_data(self, tmp_path):
        store = StateStore(tmp_path / "data.json")
        store.set("a", 1)
        result = store.update({"b": 2})
        assert result == {"a": 1, "b": 2}

    def test_path_attribute(self, tmp_path):
        p = tmp_path / "config.json"
        store = StateStore(p)
        assert store.path == p
        assert isinstance(store.path, Path)