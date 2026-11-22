"""VS Code 插件 JSON Schema 的回归测试。

`$deps.files[].src` 允许 `${VAR}` 开头的地址与相对 `$deps.base` 的路径——变量
在扫描期先展开，相对路径在计划期拼接。schema 只做提示，不能比 byk 本身更
严格，否则合法的声明会在编辑器里报「与模式不匹配」。
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


def test_src_accepts_http_variables_and_relative_paths():
    for pattern in src_patterns(load_schema()):
        regex = re.compile(pattern)
        for value in (
            "https://example.com/tool.tar.gz",
            "http://example.com/tool",
            "${url}",
            "${github}/lansend-darwin-arm64.tar.gz",
            "./tool",
            "../hhh.js",
            "bin/tool",
            "tool",
        ):
            assert regex.search(value), f"{value!r} 被 {pattern} 拒绝"


def test_src_rejects_paths_that_point_at_the_filesystem():
    for pattern in src_patterns(load_schema()):
        regex = re.compile(pattern)
        for value in ("/abs/tool", "~/tool", "C:/tool", "ftp://example.com/a"):
            assert not regex.search(value), f"{value!r} 不该被 {pattern} 接受"


def test_alias_object_accepts_paths():
    """$paths 现在可写在别名对象 / 分组上，schema 必须允许，否则编辑器会误报。"""
    paths = load_schema()["$defs"]["AliasObject"]["properties"]["$paths"]

    assert paths["type"] == "array"
    assert paths["items"] == {"type": "string"}


def test_platform_map_completes_every_platform_key():
    schema = load_schema()
    platform_map = schema["$defs"]["PlatformMap"]

    # 平台键必须写成显式 properties，编辑器才会在对象里给出候选；
    # 只挂 additionalProperties 时 JSON LS 一个键都补不出来
    assert list(platform_map["properties"]) == [
        "linux-x86_64",
        "linux-arm64",
        "darwin-x86_64",
        "darwin-arm64",
        "windows-x64",
    ]
    for key, entry in platform_map["properties"].items():
        assert entry["type"] == "string", key
        assert entry["description"], key

    # 未知键仍然只是「当前平台不匹配」，schema 不能比 byk 更严格
    assert platform_map["additionalProperties"] == {"type": "string"}


def test_var_values_may_be_a_platform_map():
    value = load_schema()["$defs"]["AliasFileObject"]["properties"]["$var"][
        "additionalProperties"
    ]["oneOf"]

    assert value[0]["type"] == "string"
    assert value[1]["$ref"] == "#/$defs/PlatformMap"


def test_deps_declares_base():
    base = load_schema()["$defs"]["AliasFileObject"]["properties"]["$deps"][
        "properties"
    ]["base"]
    regex = re.compile(base["pattern"])

    assert base["type"] == "string"
    # 三种绝对形态（URL、Unix 路径、盘符路径）与 clone 期的相对写法都放行
    for value in (
        "https://example.com/x/",
        "http://example.com/x",
        "/Users/coke/proj/",
        "D:/proj/",
        "/",
        ".",
        "..",
        "../",
        "sub/dir",
    ):
        assert regex.search(value), value
    # base 只认字面量：变量、UNC、~、query、反斜杠都提示
    for value in (
        "${CDN}",
        "https://example.com/${v}/",
        "//server/share",
        "~/x",
        "a\\b",
        "https://example.com/x/?token=1",
    ):
        assert not regex.search(value), value
