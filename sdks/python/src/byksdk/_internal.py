"""SDK 内部工具函数"""

import functools
from importlib.util import find_spec


def require_dependency(
    package: str,
    scope: str,
    version: str | None = None,
    hint: str | None = None,
) -> None:
    """校验第三方依赖是否可用，不可用时抛出带安装指引的 ImportError

    Args:
        package: pip 包名，如 "flask"、"click"
        scope: 使用说明，如 "byksdk.web"
        version: 最低版本号，如 "2.0"，会自动生成 >= 约束
        hint: 完全自定义安装命令，传入时忽略 version
    """
    if find_spec(package) is None:
        spec = f"{package}>={version}" if version else package
        install = hint or f"pip install {spec}"
        raise ImportError(
            f"{package} is required for {scope}.\n"
            f"Install it and add to your plugin dependencies:\n"
            f"    {install}\n"
            f"    # then add '{spec}' to pyproject.toml dependencies"
        )


def requires(package: str, version: str | None = None, hint: str | None = None):
    """依赖检查装饰器，调用函数前自动校验第三方包是否可用

    可叠加使用以声明多个依赖:

        @requires("pyperclip", version="1.9.0")
        @requires("click", version="8.0.0")
        def copy_to_clipboard(text): ...

    Args:
        package: pip 包名
        version: 最低版本号，自动生成 >= 约束
        hint: 完全自定义安装命令
    """
    def decorator(func):
        @functools.wraps(func)
        def wrapper(*args, **kwargs):
            scope = f"{func.__module__}.{func.__qualname__}"
            require_dependency(package, scope, version=version, hint=hint)
            return func(*args, **kwargs)
        return wrapper
    return decorator
