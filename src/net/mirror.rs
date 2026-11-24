//! 访问 GitHub 的传输通道：直连，或经 gh-proxy 反代。
//!
//! 通道是**这台机器的网络处境**，不是文档属性——所以它只来自配置，不进语法、
//! 也不进 `$deps.base`。这里只有纯判定：把地址映射成最终要请求的地址，
//! 不读配置、不碰文件系统（`net` 是叶子模块）。
//!
//! 带凭证的地址一律直连：凭证可能是 query（`?token=…`），也可能是 userinfo
//! （`user:pass@host`）。写在路径里的凭证无法判定，这条边界写在文档里。

use std::sync::atomic::{AtomicBool, Ordering};

/// gh-proxy 的反代前缀：把完整上游 URL 拼在域名后面。
pub(crate) const GH_PROXY: &str = "https://gh-proxy.com/";

/// 配置里 `net-github` 的取值。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum Channel {
    #[default]
    Direct,
    Ghproxy,
}

impl Channel {
    pub(crate) fn parse(name: &str) -> Self {
        match name {
            "ghproxy" => Self::Ghproxy,
            _ => Self::Direct,
        }
    }

    /// 这个地址该往哪请求。
    pub(crate) fn route(self, url: &str) -> Route {
        if self == Self::Direct || !is_github_file(url) {
            return Route::Direct;
        }
        if carries_credentials(url) {
            return Route::Credentials;
        }
        Route::Mirrored(format!("{GH_PROXY}{url}"))
    }
}

/// 一次请求的落点。
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Route {
    /// 按原地址请求。
    Direct,
    /// 改走镜像。
    Mirrored(String),
    /// 本该走镜像，但地址带凭证：按原地址请求，并提示一次。
    Credentials,
}

/// 只认 GitHub 的文件地址；第三方镜像地址不会再次被改写。
///
/// 按 authority 判主机（而不是前缀匹配），这样 `https://user@github.com/…`
/// 这种带 userinfo 的写法也能被认出来，再由凭证规则挡在镜像之外。
pub(crate) fn is_github_file(url: &str) -> bool {
    let Some(rest) = url.strip_prefix("https://") else {
        return false;
    };
    let authority = rest.split(['/', '?', '#']).next().unwrap_or("");
    let host = authority.rsplit('@').next().unwrap_or(authority);
    matches!(host, "github.com" | "raw.githubusercontent.com")
}

/// query 与 userinfo 都算带凭证；宁可少加速，也不把凭证交给第三方。
pub(crate) fn carries_credentials(url: &str) -> bool {
    let Some((_scheme, rest)) = url.split_once("://") else {
        return false;
    };
    if rest.contains('?') {
        return true;
    }
    let authority = rest.split(['/', '?', '#']).next().unwrap_or(rest);
    authority.contains('@')
}

/// 展示用地址：剥掉 query 与 userinfo，凭证不进日志、不进 `$deps.base`。
pub(crate) fn display_url(url: &str) -> String {
    let Some((scheme, rest)) = url.split_once("://") else {
        return url.to_string();
    };
    let authority = rest.split(['/', '?', '#']).next().unwrap_or("");
    let host = authority.rsplit('@').next().unwrap_or(authority);
    let path = rest[authority.len()..]
        .split(['?', '#'])
        .next()
        .unwrap_or("");
    format!("{scheme}://{host}{path}")
}

static CREDENTIALS_NOTED: AtomicBool = AtomicBool::new(false);

/// 每个进程只提示一次：一个文档里可能有十几条带凭证的地址。
pub(crate) fn note_credentials() {
    if CREDENTIALS_NOTED.swap(true, Ordering::Relaxed) {
        return;
    }
    eprintln!(
        "Note: a URL carries credentials, so it was fetched directly instead of through the mirror"
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    const RAW: &str = "https://raw.githubusercontent.com/u/r/HEAD/tool";
    const RELEASE: &str = "https://github.com/u/r/releases/latest/download/tool";

    #[test]
    fn parses_the_configured_names() {
        assert_eq!(Channel::parse("ghproxy"), Channel::Ghproxy);
        assert_eq!(Channel::parse("direct"), Channel::Direct);
        // 未知值由注册表挡住，这里退化成默认值
        assert_eq!(Channel::parse("jsdelivr"), Channel::Direct);
        assert_eq!(Channel::parse(""), Channel::Direct);
    }

    #[test]
    fn direct_never_rewrites() {
        assert_eq!(Channel::Direct.route(RAW), Route::Direct);
        assert_eq!(Channel::Direct.route(RELEASE), Route::Direct);
    }

    #[test]
    fn ghproxy_wraps_github_files_only() {
        assert_eq!(
            Channel::Ghproxy.route(RAW),
            Route::Mirrored(format!("{GH_PROXY}{RAW}"))
        );
        assert_eq!(
            Channel::Ghproxy.route(RELEASE),
            Route::Mirrored(format!("{GH_PROXY}{RELEASE}"))
        );

        for other in [
            "https://gitee.com/u/r/raw/HEAD/tool",
            "https://example.com/tool",
            "http://github.com/u/r/tool",
            "https://raw.githubusercontent.com.evil/u/r/tool",
            // 已经是镜像地址：不能再套一层
            "https://gh-proxy.com/https://raw.githubusercontent.com/u/r/HEAD/tool",
        ] {
            assert_eq!(Channel::Ghproxy.route(other), Route::Direct, "{other}");
        }
    }

    #[test]
    fn credentials_always_go_direct() {
        for url in [
            "https://raw.githubusercontent.com/u/r/HEAD/tool?token=ghp_x",
            "https://raw.githubusercontent.com/u/r/HEAD/tool?raw=true",
            "https://ghp_x@raw.githubusercontent.com/u/r/HEAD/tool",
            "https://user:pass@github.com/u/r/releases/download/v1/tool",
        ] {
            assert_eq!(Channel::Ghproxy.route(url), Route::Credentials, "{url}");
        }
    }

    #[test]
    fn recognises_github_file_urls() {
        assert!(is_github_file(RAW));
        assert!(is_github_file(RELEASE));
        assert!(!is_github_file("https://gitee.com/u/r/raw/HEAD/tool"));
        assert!(!is_github_file(
            "https://raw.githubusercontent.com.evil/u/r"
        ));
    }

    #[test]
    fn display_url_drops_credentials() {
        assert_eq!(
            display_url("https://raw.githubusercontent.com/u/r/HEAD/tool?token=ghp_x"),
            "https://raw.githubusercontent.com/u/r/HEAD/tool"
        );
        assert_eq!(
            display_url("https://ghp_x@github.com/u/r/releases/download/v1/tool"),
            "https://github.com/u/r/releases/download/v1/tool"
        );
        assert_eq!(
            display_url("https://example.com/x#frag"),
            "https://example.com/x"
        );
        assert_eq!(display_url("tool"), "tool");
    }

    #[test]
    fn credential_check_ignores_paths_without_authority() {
        assert!(!carries_credentials("tool"));
        assert!(!carries_credentials(""));
        assert!(!carries_credentials("https://example.com/a@b"));
        assert!(carries_credentials("https://example.com/x?y=1"));
    }
}
