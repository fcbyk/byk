//! 下载层的 CDN 路由：沿用 clone 落盘的 `$deps.base`。
//!
//! 只认 gh-proxy：`base` 以它开头，说明这份文件是从加速通道克隆的，作者写死的
//! GitHub 绝对地址也应走同一条通道。jsdelivr 只镜像仓库文件，Release 资产无法
//! 经它取回，所以不做猜测。

use crate::deps::types::Base;

/// gh-proxy 前缀；clone 与下载层共用这一处字面量。
const GH_PROXY_PREFIX: &str = "https://gh-proxy.com/";

/// 把上游 URL 拼到 gh-proxy 后面。
pub(crate) fn gh_proxy_url(url: &str) -> String {
    format!("{GH_PROXY_PREFIX}{url}")
}

/// 下载时是否改走加速通道；由 `$deps.base` 推断。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct CdnRoute {
    gh_proxy: bool,
}

impl CdnRoute {
    pub(crate) fn from_base(base: Option<&Base>) -> Self {
        Self {
            gh_proxy: matches!(base, Some(Base::Url(url)) if url.starts_with(GH_PROXY_PREFIX)),
        }
    }

    /// 绝对 GitHub 地址改走 gh-proxy；已经带前缀的不重复包，其他地址原样返回。
    pub(crate) fn url(self, url: &str) -> String {
        let github = url.starts_with("https://github.com/")
            || url.starts_with("https://raw.githubusercontent.com/");
        if self.gh_proxy && github && !url.starts_with(GH_PROXY_PREFIX) {
            gh_proxy_url(url)
        } else {
            url.to_string()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ghproxy_base() -> Base {
        Base::Url("https://gh-proxy.com/https://raw.githubusercontent.com/u/r/HEAD/".to_string())
    }

    #[test]
    fn ghproxy_base_routes_absolute_github_urls() {
        let route = CdnRoute::from_base(Some(&ghproxy_base()));
        for url in [
            "https://github.com/u/r/releases/latest/download/tool.zip",
            "https://raw.githubusercontent.com/u/r/HEAD/tool",
        ] {
            assert_eq!(route.url(url), format!("https://gh-proxy.com/{url}"));
        }
    }

    #[test]
    fn ghproxy_base_leaves_other_and_already_routed_urls_alone() {
        let route = CdnRoute::from_base(Some(&ghproxy_base()));
        for url in [
            "https://gh-proxy.com/https://raw.githubusercontent.com/u/r/HEAD/tool",
            "https://example.com/tool",
            "http://github.com/u/r/tool",
            "https://gitee.com/u/r/raw/HEAD/tool",
        ] {
            assert_eq!(route.url(url), url);
        }
    }

    #[test]
    fn other_bases_do_not_route() {
        for base in [
            None,
            Some(Base::Url(
                "https://raw.githubusercontent.com/u/r/HEAD/".to_string(),
            )),
            Some(Base::Url(
                "https://cdn.jsdelivr.net/gh/u/r@HEAD/".to_string(),
            )),
            Some(Base::Dir("C:/tools/".to_string())),
        ] {
            let route = CdnRoute::from_base(base.as_ref());
            let url = "https://github.com/u/r/releases/latest/download/tool.zip";
            assert_eq!(route.url(url), url);
        }
    }

    #[test]
    fn routing_is_idempotent() {
        let route = CdnRoute::from_base(Some(&ghproxy_base()));
        let once = route.url("https://github.com/u/r/tool");
        assert_eq!(route.url(&once), once);
    }
}
