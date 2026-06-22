"""测试 paths.py 模块"""

from pathlib import Path

from byksdk import paths


class TestPathConstants:
    """测试路径常量"""

    def test_root_dir_is_path(self):
        assert isinstance(paths.ROOT_DIR, Path)

    def test_state_dir_under_root(self):
        assert paths.STATE_DIR == paths.ROOT_DIR / "state"

    def test_plugins_dir_under_root(self):
        assert paths.PLUGINS_DIR == paths.ROOT_DIR / "plugins"

    def test_logs_dir_under_root(self):
        assert paths.LOGS_DIR == paths.ROOT_DIR / "logs"

    def test_runtime_dir_under_root(self):
        assert paths.RUNTIME_DIR == paths.ROOT_DIR / "runtime"

    def test_cache_dir_under_root(self):
        assert paths.CACHE_DIR == paths.ROOT_DIR / "cache"

    def test_plugins_log_file_under_logs(self):
        assert paths.PLUGINS_LOG_FILE == paths.LOGS_DIR / "plugins.log"


class TestEnsureDirs:
    """测试 ensure_dirs 函数"""

    def test_creates_all_directories(self, tmp_path, monkeypatch):
        monkeypatch.setattr(paths, "ROOT_DIR", tmp_path / ".byk")
        monkeypatch.setattr(paths, "STATE_DIR", tmp_path / ".byk" / "state")
        monkeypatch.setattr(paths, "PLUGINS_DIR", tmp_path / ".byk" / "plugins")
        monkeypatch.setattr(paths, "LOGS_DIR", tmp_path / ".byk" / "logs")
        monkeypatch.setattr(paths, "RUNTIME_DIR", tmp_path / ".byk" / "runtime")
        monkeypatch.setattr(paths, "CACHE_DIR", tmp_path / ".byk" / "cache")

        paths.ensure_dirs()

        assert paths.ROOT_DIR.is_dir()
        assert paths.STATE_DIR.is_dir()
        assert paths.PLUGINS_DIR.is_dir()
        assert paths.LOGS_DIR.is_dir()
        assert paths.RUNTIME_DIR.is_dir()
        assert paths.CACHE_DIR.is_dir()

    def test_idempotent(self, tmp_path, monkeypatch):
        monkeypatch.setattr(paths, "ROOT_DIR", tmp_path / ".byk")
        monkeypatch.setattr(paths, "STATE_DIR", tmp_path / ".byk" / "state")
        monkeypatch.setattr(paths, "PLUGINS_DIR", tmp_path / ".byk" / "plugins")
        monkeypatch.setattr(paths, "LOGS_DIR", tmp_path / ".byk" / "logs")
        monkeypatch.setattr(paths, "RUNTIME_DIR", tmp_path / ".byk" / "runtime")
        monkeypatch.setattr(paths, "CACHE_DIR", tmp_path / ".byk" / "cache")

        paths.ensure_dirs()
        paths.ensure_dirs()  # 重复调用不报错

        assert paths.ROOT_DIR.is_dir()