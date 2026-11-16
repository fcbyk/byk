"""VS Code 插件 JSON Schema 的回归测试。

`$deps.files[].src` 允许写 `${VAR}` 开头的 URL —— 变量在扫描期先展开，再校验
是不是 http(s)。schema 只做提示，不能比 byk 本身更严格，否则合法的声明会
在编辑器里报「与 ^https?:// 的模式不匹配」。
"""

import json
import re
from pathlib import Path

SCHEMA = (
    Path(__file__).resolve().parents[1]
    / "extensions"
    / "vscode"
    / "schemas"
    / "byk-alias.json"
)


def load_schema():
    return json.loads(SCHEMA.read_text(encoding="utf-8"))


def src_patterns(schema):
    """对象写法的 src，以及字符串简写那条。"""
    object_form = schema["$defs"]["DepFileEntry"]["properties"]["src"]
    shorthand = schema["$defs"]["AliasFileObject"]["properties"]["$deps"][
        "properties"
    ]["files"]["items"]["oneOf"][0]
    return [object_form["pattern"], shorthand["pattern"]]


def test_schema_is_valid_json_with_deps_definitions():
    schema = load_schema()

    assert schema["$schema"].startswith("http://json-schema.org/")
    for name in ("AliasFileObject", "LinkFile", "AliasObject", "DepFileEntry"):
        assert name in schema["$defs"], name


def test_src_accepts_http_and_variables():
    for pattern in src_patterns(load_schema()):
        regex = re.compile(pattern)
        for value in (
            "https://example.com/tool.tar.gz",
            "http://example.com/tool",
            "${url}",
            "${github}/lansend-darwin-arm64.tar.gz",
        ):
            assert regex.search(value), f"{value!r} 被 {pattern} 拒绝"


def test_src_still_rejects_relative_paths_and_other_schemes():
    for pattern in src_patterns(load_schema()):
        regex = re.compile(pattern)
        for value in ("./tool", "/abs/tool", "ftp://example.com/a", "tool"):
            assert not regex.search(value), f"{value!r} 不该被 {pattern} 接受"
