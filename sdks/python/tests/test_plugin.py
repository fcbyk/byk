"""测试 plugin.py 模块"""

import sys
from pathlib import Path

from byksdk.plugin import PluginContext, plugin
from byksdk.state import StateStore


class TestPlugin:
    """测试 plugin 函数"""

    def test_returns_plugin_context(self, tmp_path, monkeypatch):
        monkeypatch.setattr(sys.modules["byksdk.plugin"], "PLUGINS_DIR", tmp_path / "plugins")
        monkeypatch.setattr(sys.modules["byksdk.logging"], "PLUGINS_DIR", tmp_path / "plugins")

        ctx = plugin("server")
        assert isinstance(ctx, PluginContext)
        assert ctx.name == "server"
        assert ctx.home == tmp_path / "plugins" / "server"

    def test_home_is_correct_path(self, tmp_path, monkeypatch):
        monkeypatch.setattr(sys.modules["byksdk.plugin"], "PLUGINS_DIR", tmp_path / "plugins")
        monkeypatch.setattr(sys.modules["byksdk.logging"], "PLUGINS_DIR", tmp_path / "plugins")

        ctx = plugin("server")
        assert isinstance(ctx.home, Path)
        assert ctx.home == tmp_path / "plugins" / "server"

    def test_state_default(self, tmp_path, monkeypatch):
        monkeypatch.setattr(sys.modules["byksdk.plugin"], "PLUGINS_DIR", tmp_path / "plugins")
        monkeypatch.setattr(sys.modules["byksdk.logging"], "PLUGINS_DIR", tmp_path / "plugins")

        ctx = plugin("server")
        s = ctx.state()
        assert isinstance(s, StateStore)
        assert s.path == tmp_path / "plugins" / "server" / "state.json"

    def test_state_named(self, tmp_path, monkeypatch):
        monkeypatch.setattr(sys.modules["byksdk.plugin"], "PLUGINS_DIR", tmp_path / "plugins")
        monkeypatch.setattr(sys.modules["byksdk.logging"], "PLUGINS_DIR", tmp_path / "plugins")

        ctx = plugin("server")
        s = ctx.state("config")
        assert s.path == tmp_path / "plugins" / "server" / "config.json"

    def test_state_multiple_names(self, tmp_path, monkeypatch):
        monkeypatch.setattr(sys.modules["byksdk.plugin"], "PLUGINS_DIR", tmp_path / "plugins")
        monkeypatch.setattr(sys.modules["byksdk.logging"], "PLUGINS_DIR", tmp_path / "plugins")

        ctx = plugin("server")
        ctx.state("config").set("timeout", 30)
        ctx.state("cache").set("last_check", "2024-01-01")
        assert ctx.state("config").get("timeout") == 30
        assert ctx.state("cache").get("last_check") == "2024-01-01"

    def test_same_name_gives_same_state(self, tmp_path, monkeypatch):
        monkeypatch.setattr(sys.modules["byksdk.plugin"], "PLUGINS_DIR", tmp_path / "plugins")
        monkeypatch.setattr(sys.modules["byksdk.logging"], "PLUGINS_DIR", tmp_path / "plugins")

        ctx1 = plugin("server")
        ctx2 = plugin("server")
        ctx1.state().set("port", 8080)
        assert ctx2.state().get("port") == 8080

    def test_different_names_isolated(self, tmp_path, monkeypatch):
        monkeypatch.setattr(sys.modules["byksdk.plugin"], "PLUGINS_DIR", tmp_path / "plugins")
        monkeypatch.setattr(sys.modules["byksdk.logging"], "PLUGINS_DIR", tmp_path / "plugins")

        a = plugin("a")
        b = plugin("b")
        a.state().set("key", "value_a")
        b.state().set("key", "value_b")
        assert a.state().get("key") == "value_a"
        assert b.state().get("key") == "value_b"

    def test_logger_name(self, tmp_path, monkeypatch):
        monkeypatch.setattr(sys.modules["byksdk.plugin"], "PLUGINS_DIR", tmp_path / "plugins")
        monkeypatch.setattr(sys.modules["byksdk.logging"], "PLUGINS_DIR", tmp_path / "plugins")

        ctx = plugin("server")
        assert ctx.logger.name == "byk.server"

    def test_app_store_default(self, tmp_path, monkeypatch):
        monkeypatch.setattr(sys.modules["byksdk.plugin"], "PLUGINS_DIR", tmp_path / "plugins")
        monkeypatch.setattr(sys.modules["byksdk.plugin"], "STATE_DIR", tmp_path / "state")
        monkeypatch.setattr(sys.modules["byksdk.logging"], "PLUGINS_DIR", tmp_path / "plugins")

        ctx = plugin("server")
        s = ctx.app.store()
        assert isinstance(s, StateStore)
        assert s.path == tmp_path / "state" / "plugins.state.json"

    def test_app_store_named(self, tmp_path, monkeypatch):
        monkeypatch.setattr(sys.modules["byksdk.plugin"], "PLUGINS_DIR", tmp_path / "plugins")
        monkeypatch.setattr(sys.modules["byksdk.plugin"], "STATE_DIR", tmp_path / "state")
        monkeypatch.setattr(sys.modules["byksdk.logging"], "PLUGINS_DIR", tmp_path / "plugins")

        ctx = plugin("server")
        s = ctx.app.store("shared")
        assert s.path == tmp_path / "state" / "shared.json"

    def test_app_store_shared_across_plugins(self, tmp_path, monkeypatch):
        monkeypatch.setattr(sys.modules["byksdk.plugin"], "PLUGINS_DIR", tmp_path / "plugins")
        monkeypatch.setattr(sys.modules["byksdk.plugin"], "STATE_DIR", tmp_path / "state")
        monkeypatch.setattr(sys.modules["byksdk.logging"], "PLUGINS_DIR", tmp_path / "plugins")

        a = plugin("a")
        b = plugin("b")
        a.app.store("shared").set("key", "value")
        assert b.app.store("shared").get("key") == "value"

    def test_app_logger(self, tmp_path, monkeypatch):
        monkeypatch.setattr(sys.modules["byksdk.plugin"], "PLUGINS_DIR", tmp_path / "plugins")
        monkeypatch.setattr(sys.modules["byksdk.logging"], "PLUGINS_DIR", tmp_path / "plugins")

        ctx = plugin("server")
        assert ctx.app.logger.name == "byk"