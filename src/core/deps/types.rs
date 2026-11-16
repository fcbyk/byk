//! `$deps` 的数据模型；这些类型在解析、计划和同步清单之间共享。

use std::path::PathBuf;

use super::path::path_to_slash;

/// 一份完整的 `$deps` 声明：基准目录加条目列表。
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Declaration {
    /// 相对 `src` 的解析基准。落盘态只可能是绝对地址，见 [`Base`]。
    pub(crate) base: Option<Base>,
    pub(crate) files: Vec<DepEntry>,
    /// pip 包声明（PEP 508/440 规格字符串），装进专属目录的虚拟环境。
    pub(crate) pip: Vec<String>,
    /// npm 包声明（包名 + 可选版本约束），装进专属目录的 node_modules。
    pub(crate) npm: Vec<String>,
}

/// `$deps.base` 的两种落盘形态。
///
/// 两者都是**目录**，都必定以 `/` 结尾：远程 clone 写出 `Url`，本地 clone
/// 写出 `Dir`。形态决定 `src` 怎么拼、执行层用 `curl` 还是 `fs::copy`。
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Base {
    /// 绝对 http(s) 目录 URL。
    Url(String),
    /// 本机绝对目录路径，用 `/` 分隔（Windows 也是）。
    Dir(String),
}

impl Base {
    pub(crate) fn as_str(&self) -> &str {
        match self {
            Self::Url(text) | Self::Dir(text) => text,
        }
    }
}

/// 一个依赖条目：语法层校验后的产物，不碰文件系统。
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct DepEntry {
    pub(crate) index: usize,
    /// 绝对 http(s) URL，或相对 `$deps.base` 的路径（原样保留，计划期拼接）。
    pub(crate) src: String,
    /// 相对专属目录的子目录段，空切片表示根。
    pub(crate) to: Vec<String>,
    /// 落点文件名；缺省从 `src` 推导。
    pub(crate) name: Option<String>,
    /// 把 `src` 当压缩包解压到 `to`，而不是落盘压缩包本身。
    pub(crate) extract: bool,
    /// 非 Windows 补 `chmod 755`。
    pub(crate) exec: bool,
}

/// 计划层产物：绝对落点已确定。
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct PlannedAsset {
    pub(crate) index: usize,
    /// 展示用标签：`<key>/<to>/<name>`。
    pub(crate) label: String,
    /// 解析后的绝对来源。
    pub(crate) origin: Origin,
    pub(crate) dest: PathBuf,
    /// 解压条目；`dest` 是目标目录本身。
    pub(crate) archive: bool,
    pub(crate) exec: bool,
}

/// 一个资产的绝对来源：远程 URL，或本机文件。
///
/// 形态由 `base` 的类型决定，`src` 自己不表态——绝对 URL 的 `src` 永远走到
/// [`Origin::Url`]，只有相对 `src` 才可能被本地 base 拼成本机路径。
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Origin {
    Url(String),
    File(PathBuf),
}

impl Origin {
    /// 展示与清单共用的字面量。本地路径统一 `/` 分隔，Windows 也一样。
    pub(crate) fn as_str(&self) -> String {
        match self {
            Self::Url(url) => url.clone(),
            Self::File(path) => path_to_slash(path),
        }
    }

    /// 来源是本机文件吗。这类条目走 `fs::copy`，不碰网络——汇总行据此
    /// 注明其中几条来自本地。
    pub(crate) fn is_local(&self) -> bool {
        matches!(self, Self::File(_))
    }
}
