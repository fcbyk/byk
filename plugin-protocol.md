# 插件协议设计

byk 插件协议的本质：**告诉 CLI 怎么运行你**。

## 结构

```json
{
  "<plugin-key>": {
    "<behavior-type>": {
      // 安装配置
      "commands": {
        "<command-name>": {
          // 命令定义
        }
      }
    }
  }
}
```

- **plugin-key**：插件唯一标识，即 `byk add <key>` 中的 `<key>`
- **behavior-type**：行为类型，编码 **运行时生态 + 运行模式**
- **commands**：该插件注册的命令列表

## 命名规则

行为类型 = `<生态>-<模式>`

| 生态 | 含义 | 安装方式 |
|------|------|----------|
| `py` | Python 生态 | pip |
| `js` | JavaScript 生态 | npm |
| `bin` | 无运行时（纯二进制） | 直接下载 |

| 模式 | 含义 | 执行方式 |
|------|------|----------|
| `-m` | 模块（module） | `<runtime> -m <module>` |
| `-f` | 文件（file） | `<runtime> <file>` |
| `-b` | 二进制（binary） | 直接执行 |

> 示例：`py-m` = Python 生态 + 模块模式 → `python -m <module>`

## 行为类型

### py-m — Python 模块模式

通过 pip 安装，执行时调用 `python -m <module>`。

```json
{
  "hello": {
    "py-m": {
      "pip": "byk-hello",
      "url": "https://...",
      "commands": {
        "hello": {
          "module": "hello.one",
          "description": "Example subcommand"
        }
      }
    }
  }
}
```

| 字段 | 类型 | 必填 | 说明 |
|------|------|------|------|
| `pip` | string | 是* | pip 包名，用于 `pip install <pip>` |
| `url` | string | 否 | 远程安装地址（覆盖 `pip`） |
| `local` | string | 否 | 本地安装路径（`--file` 模式） |
| `pyproject` | string | 否 | pyproject.toml 相对目录（`-e` 模式） |
| `commands` | object | 是 | 注册的命令列表 |

\* 可编辑安装模式（`-e`）下 `pip` 可选，其余模式必填。

**commands 字段：**

| 字段 | 类型 | 必填 | 说明 |
|------|------|------|------|
| `module` | string | 是 | Python 模块路径，执行 `python -m <module>` |
| `description` | string | 否 | 命令描述 |

### py-f — Python 脚本模式（规划中）

通过 pip 安装，执行时调用 `python <file>`。

```json
{
  "my-tool": {
    "py-f": {
      "pip": "byk-mytool",
      "commands": {
        "run": {
          "file": "mytool.py",
          "description": "Run the tool"
        }
      }
    }
  }
}
```

| 字段 | 类型 | 必填 | 说明 |
|------|------|------|------|
| `pip` | string | 是* | pip 包名 |
| `url` | string | 否 | 远程安装地址 |
| `local` | string | 否 | 本地安装路径 |
| `pyproject` | string | 否 | pyproject.toml 相对目录 |
| `commands` | object | 是 | 注册的命令列表 |

**commands 字段：**

| 字段 | 类型 | 必填 | 说明 |
|------|------|------|------|
| `file` | string | 是 | Python 脚本路径，执行 `python <file>` |
| `description` | string | 否 | 命令描述 |

### py-b — Python 生态二进制模式（规划中）

通过 pip 安装，entry_points 生成的二进制，直接执行（无需 Python 运行时）。

```json
{
  "deploy": {
    "py-b": {
      "pip": "byk-deploy",
      "commands": {
        "deploy": {
          "bin": "byk-deploy",
          "description": "Deploy services"
        }
      }
    }
  }
}
```

| 字段 | 类型 | 必填 | 说明 |
|------|------|------|------|
| `pip` | string | 是 | pip 包名 |
| `url` | string | 否 | 远程安装地址 |
| `commands` | object | 是 | 注册的命令列表 |

**commands 字段：**

| 字段 | 类型 | 必填 | 说明 |
|------|------|------|------|
| `bin` | string | 是 | 二进制文件名，从 venv/bin 直接执行 |
| `description` | string | 否 | 命令描述 |

### bin — 纯二进制模式（规划中）

无运行时依赖，直接下载执行（Go / Rust / C 编译产物）。

```json
{
  "my-tool": {
    "bin": {
      "url": "https://...",
      "commands": {
        "my-tool": {
          "bin": "my-tool",
          "description": "Run my tool"
        }
      }
    }
  }
}
```

| 字段 | 类型 | 必填 | 说明 |
|------|------|------|------|
| `url` | string | 是 | 下载地址 |
| `commands` | object | 是 | 注册的命令列表 |

### js-m / js-f / js-b（远期规划）

JavaScript 生态的三种模式，对应 `py-m` / `py-f` / `py-b`，安装方式为 `npm`。

## 设计原则

1. **行为 key 自描述**：`<生态>-<模式>` 完整表达"怎么运行你"
2. **安装与运行分离**：安装方式（`pip`/`npm`/`url`）是内部字段，不污染行为 key
3. **有运行时用生态前缀，无运行时用 `bin`**：不做无意义的统一
4. **commands 的目标字段与模式对应**：`-m` → `module`，`-f` → `file`，`-b` → `bin`

## 变迁记录

| 版本 | 变更 |
|------|------|
| 当前 | 行为类型 `py-m`，`pip` 字段；规划中 `py-f`、`py-b`、`bin` |