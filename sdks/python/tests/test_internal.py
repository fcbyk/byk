"""测试 _internal.py 模块"""

import pytest
from byksdk._internal import require_dependency, requires


class TestRequireDependency:
    """测试 require_dependency 函数"""

    def test_dependency_available(self, mock_find_spec):
        """依赖已安装时不抛异常"""
        mock_find_spec.return_value = True
        require_dependency("flask", "byksdk.web")

    def test_dependency_missing_with_version(self, mock_find_spec):
        """依赖缺失时抛出带版本号的 ImportError"""
        mock_find_spec.return_value = None
        with pytest.raises(ImportError) as exc_info:
            require_dependency("flask", "byksdk.web", version="2.0")
        assert "flask>=2.0" in str(exc_info.value)
        assert "byksdk.web" in str(exc_info.value)

    def test_dependency_missing_without_version(self, mock_find_spec):
        """依赖缺失时不带版本号"""
        mock_find_spec.return_value = None
        with pytest.raises(ImportError) as exc_info:
            require_dependency("click", "byksdk.cli")
        assert "click" in str(exc_info.value)
        assert "byksdk.cli" in str(exc_info.value)

    def test_dependency_missing_with_custom_hint(self, mock_find_spec):
        """自定义安装提示"""
        mock_find_spec.return_value = None
        with pytest.raises(ImportError) as exc_info:
            require_dependency(
                "custom-pkg", "byksdk.custom",
                hint="pip install custom-pkg --extra-index-url https://example.com"
            )
        assert "custom-pkg --extra-index-url" in str(exc_info.value)


class TestRequiresDecorator:
    """测试 requires 装饰器"""

    def test_decorator_passes_when_available(self, mock_find_spec):
        """依赖可用时函数正常执行"""
        mock_find_spec.return_value = True

        @requires("flask", version="2.0")
        def my_func():
            return "ok"

        assert my_func() == "ok"

    def test_decorator_raises_when_missing(self, mock_find_spec):
        """依赖缺失时抛出 ImportError"""
        mock_find_spec.return_value = None

        @requires("flask", version="2.0")
        def my_func():
            return "ok"

        with pytest.raises(ImportError):
            my_func()

    def test_decorator_preserves_metadata(self, mock_find_spec):
        """装饰器保留原函数的元数据"""
        mock_find_spec.return_value = True

        @requires("flask")
        def my_func():
            """docstring"""
            return "ok"

        assert my_func.__name__ == "my_func"
        assert my_func.__doc__ == "docstring"

    def test_multiple_decorators(self, mock_find_spec):
        """多个 requires 装饰器叠加"""
        mock_find_spec.return_value = True

        @requires("pyperclip", version="1.9.0")
        @requires("click", version="8.0.0")
        def my_func():
            return "ok"

        assert my_func() == "ok"

    def test_multiple_decorators_first_missing(self, mock_find_spec):
        """多个装饰器中第一个依赖缺失就报错"""
        mock_find_spec.return_value = None

        @requires("pyperclip", version="1.9.0")
        @requires("click", version="8.0.0")
        def my_func():
            return "ok"

        with pytest.raises(ImportError) as exc_info:
            my_func()
        assert "pyperclip" in str(exc_info.value)