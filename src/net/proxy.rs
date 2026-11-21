//! curl 的代理选择：`BYK_USE_PROXY=false` 强制直连；否则 `BYK_PROXY` 优先，
//! 其次是已有环境变量，Windows 静态系统代理兜底。
//!
//! 只作用于 byk 发起的 curl 子进程。自动探测延迟到第一次取 curl 时才发生，
//! 所以 `-h`、`--check` 这类命令不会去读注册表。

use std::process::Command;
use std::sync::OnceLock;

#[derive(Clone, Debug, Default, PartialEq, Eq)]
struct ProxyEnv {
    http: Option<String>,
    https: Option<String>,
    all: Option<String>,
    no_proxy: Option<String>,
}

impl ProxyEnv {
    fn apply_to(&self, command: &mut Command) {
        for (key, value) in [
            ("http_proxy", &self.http),
            ("https_proxy", &self.https),
            ("all_proxy", &self.all),
        ] {
            if let Some(value) = value {
                command.env(key, value);
            }
        }
        if let Some(value) = &self.no_proxy {
            command.env("no_proxy", value);
        }
    }
}

#[derive(Clone, Debug)]
enum Mode {
    /// 不注入：沿用环境变量与 curlrc 里已有的配置。
    Inherit,
    /// `BYK_USE_PROXY=false`：命令行强制直连，压过环境变量与 curlrc。
    Direct,
    /// 单一代理地址：用命令行参数覆盖 curlrc。
    Proxy {
        url: String,
        no_proxy: Option<String>,
    },
    /// 协议分开的代理只能通过环境变量表达。
    Env(ProxyEnv),
}

static MODE: OnceLock<Mode> = OnceLock::new();

/// 给一个即将启动的 curl 子进程套用代理。
pub(crate) fn apply(command: &mut Command) {
    apply_mode(mode(), command);
}

fn apply_mode(mode: &Mode, command: &mut Command) {
    match mode {
        Mode::Inherit => {}
        Mode::Direct => {
            remove_proxy_env(command);
            remove_no_proxy_env(command);
            command.args(["--noproxy", "*"]);
        }
        Mode::Proxy { url, no_proxy } => {
            remove_proxy_env(command);
            command.args(["--proxy", url]);
            if let Some(list) = no_proxy {
                command.args(["--noproxy", list]);
            }
        }
        Mode::Env(env) => {
            remove_proxy_env(command);
            if env.no_proxy.is_some() {
                remove_no_proxy_env(command);
            }
            env.apply_to(command);
        }
    }
}

fn mode() -> &'static Mode {
    MODE.get_or_init(resolve)
}

fn resolve() -> Mode {
    if proxy_disabled() {
        return Mode::Direct;
    }
    if let Some(url) = env_non_empty("BYK_PROXY") {
        return Mode::Proxy {
            url,
            no_proxy: None,
        };
    }
    if proxy_env_present() {
        return Mode::Inherit;
    }
    #[cfg(windows)]
    if let Some(mode) = windows::system() {
        return mode;
    }
    Mode::Inherit
}

fn proxy_disabled() -> bool {
    std::env::var("BYK_USE_PROXY")
        .ok()
        .is_some_and(|value| is_disabled_value(&value))
}

fn is_disabled_value(value: &str) -> bool {
    matches!(
        value.trim().to_ascii_lowercase().as_str(),
        "false" | "0" | "no" | "off"
    )
}

fn env_non_empty(name: &str) -> Option<String> {
    std::env::var(name)
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

fn proxy_env_present() -> bool {
    [
        "https_proxy",
        "HTTPS_PROXY",
        "http_proxy",
        "HTTP_PROXY",
        "all_proxy",
        "ALL_PROXY",
    ]
    .iter()
    .any(|key| std::env::var_os(key).is_some_and(|value| !value.is_empty()))
}

fn remove_proxy_env(command: &mut Command) {
    for key in [
        "http_proxy",
        "HTTP_PROXY",
        "https_proxy",
        "HTTPS_PROXY",
        "all_proxy",
        "ALL_PROXY",
    ] {
        command.env_remove(key);
    }
}

fn remove_no_proxy_env(command: &mut Command) {
    for key in ["no_proxy", "NO_PROXY"] {
        command.env_remove(key);
    }
}

/// `ProxyServer` 有两种形态：裸 `host:port`（所有协议），或 `http=...;https=...;socks=...`。
fn parse_server(raw: &str) -> Option<ProxyEnv> {
    let raw = raw.trim();
    if raw.is_empty() {
        return None;
    }

    if !raw.contains('=') {
        if raw.contains(';') {
            return None;
        }
        let url = with_scheme(raw, "http");
        return Some(ProxyEnv {
            http: Some(url.clone()),
            https: Some(url),
            ..ProxyEnv::default()
        });
    }

    let mut env = ProxyEnv::default();
    for part in raw.split(';') {
        let Some((kind, value)) = part.split_once('=') else {
            continue;
        };
        let value = value.trim();
        if value.is_empty() {
            continue;
        }
        match kind.trim().to_ascii_lowercase().as_str() {
            "http" => env.http = Some(with_scheme(value, "http")),
            "https" => env.https = Some(with_scheme(value, "http")),
            "socks" | "socks5" => env.all = Some(with_scheme(value, "socks5h")),
            "socks4" => env.all = Some(with_scheme(value, "socks4")),
            _ => {}
        }
    }

    (env.http.is_some() || env.https.is_some() || env.all.is_some()).then_some(env)
}

fn with_scheme(value: &str, scheme: &str) -> String {
    if value.contains("://") {
        value.to_string()
    } else {
        format!("{scheme}://{value}")
    }
}

/// `ProxyOverride` 转成 curl 的 `no_proxy`；`<local>` 展开成回环地址。
fn parse_bypass(raw: &str) -> Option<String> {
    let mut hosts: Vec<String> = Vec::new();
    let mut local = false;
    for entry in raw.split([';', ',']) {
        let entry = entry.trim();
        if entry.is_empty() {
            continue;
        }
        if entry.eq_ignore_ascii_case("<local>") {
            local = true;
            continue;
        }
        push_unique(&mut hosts, entry);
    }
    if hosts.is_empty() && !local {
        return None;
    }
    for loopback in ["localhost", "127.0.0.1", "::1"] {
        push_unique(&mut hosts, loopback);
    }
    Some(hosts.join(","))
}

fn push_unique(hosts: &mut Vec<String>, value: &str) {
    if !hosts.iter().any(|seen| seen.eq_ignore_ascii_case(value)) {
        hosts.push(value.to_string());
    }
}

#[cfg(windows)]
mod windows {
    use super::{Mode, parse_bypass, parse_server};
    use windows_sys::Win32::Foundation::{ERROR_MORE_DATA, ERROR_SUCCESS};
    use windows_sys::Win32::System::Registry::{
        HKEY_CURRENT_USER, RRF_RT_REG_DWORD, RRF_RT_REG_SZ, RegGetValueW,
    };

    const INTERNET_SETTINGS: &str =
        "Software\\Microsoft\\Windows\\CurrentVersion\\Internet Settings";

    pub(super) fn system() -> Option<Mode> {
        if read_string("AutoConfigURL").is_some_and(|value| !value.trim().is_empty()) {
            eprintln!(
                "Warning: Windows uses automatic proxy configuration (PAC), which curl cannot evaluate; set BYK_PROXY"
            );
            return None;
        }
        if read_dword("AutoDetect") == Some(1) {
            eprintln!(
                "Warning: Windows uses automatic proxy detection (WPAD), which curl cannot evaluate; set BYK_PROXY"
            );
            return None;
        }
        if read_dword("ProxyEnable") != Some(1) {
            return None;
        }

        let raw = read_string("ProxyServer")?;
        let Some(mut env) = parse_server(&raw) else {
            eprintln!(
                "Warning: cannot understand the Windows proxy setting \"{}\"; ignoring it. Set BYK_PROXY to override",
                raw.trim()
            );
            return None;
        };
        env.no_proxy = read_string("ProxyOverride")
            .as_deref()
            .and_then(parse_bypass);
        Some(system_mode(env))
    }

    /// 单一地址能完整表达时走命令行参数，从而压过 curlrc。
    fn system_mode(env: super::ProxyEnv) -> Mode {
        match (&env.http, &env.https, &env.all) {
            (Some(http), Some(https), _) if http == https => Mode::Proxy {
                url: http.clone(),
                no_proxy: env.no_proxy,
            },
            (None, None, Some(all)) => Mode::Proxy {
                url: all.clone(),
                no_proxy: env.no_proxy,
            },
            _ => Mode::Env(env),
        }
    }

    fn read_dword(name: &str) -> Option<u32> {
        let key = wide(INTERNET_SETTINGS);
        let value = wide(name);
        let mut data: u32 = 0;
        let mut size = std::mem::size_of::<u32>() as u32;
        let status = unsafe {
            RegGetValueW(
                HKEY_CURRENT_USER,
                key.as_ptr(),
                value.as_ptr(),
                RRF_RT_REG_DWORD,
                std::ptr::null_mut(),
                (&mut data as *mut u32).cast(),
                &mut size,
            )
        };
        (status == ERROR_SUCCESS).then_some(data)
    }

    fn read_string(name: &str) -> Option<String> {
        let key = wide(INTERNET_SETTINGS);
        let value = wide(name);
        let mut size: u32 = 0;
        let status = unsafe {
            RegGetValueW(
                HKEY_CURRENT_USER,
                key.as_ptr(),
                value.as_ptr(),
                RRF_RT_REG_SZ,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                &mut size,
            )
        };
        if status != ERROR_SUCCESS && status != ERROR_MORE_DATA {
            return None;
        }
        if size < 2 {
            return Some(String::new());
        }

        let mut buffer = vec![0u16; (size as usize).div_ceil(2)];
        let mut data_size = (buffer.len() * std::mem::size_of::<u16>()) as u32;
        let status = unsafe {
            RegGetValueW(
                HKEY_CURRENT_USER,
                key.as_ptr(),
                value.as_ptr(),
                RRF_RT_REG_SZ,
                std::ptr::null_mut(),
                buffer.as_mut_ptr().cast(),
                &mut data_size,
            )
        };
        if status != ERROR_SUCCESS {
            return None;
        }
        let len = buffer
            .iter()
            .position(|unit| *unit == 0)
            .unwrap_or(buffer.len());
        Some(String::from_utf16_lossy(&buffer[..len]))
    }

    fn wide(text: &str) -> Vec<u16> {
        text.encode_utf16().chain(std::iter::once(0)).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn server_without_keys_applies_to_both_schemes() {
        let env = parse_server("127.0.0.1:10808").unwrap();
        assert_eq!(env.http.as_deref(), Some("http://127.0.0.1:10808"));
        assert_eq!(env.https.as_deref(), Some("http://127.0.0.1:10808"));
        assert_eq!(env.all, None);
    }

    #[test]
    fn server_with_keys_maps_each_protocol() {
        let env =
            parse_server("http=10.0.0.1:8080;https=10.0.0.1:8443;socks=127.0.0.1:1080").unwrap();
        assert_eq!(env.http.as_deref(), Some("http://10.0.0.1:8080"));
        assert_eq!(env.https.as_deref(), Some("http://10.0.0.1:8443"));
        assert_eq!(env.all.as_deref(), Some("socks5h://127.0.0.1:1080"));
    }

    #[test]
    fn server_keeps_explicit_schemes_and_ignores_empty() {
        let env = parse_server("https=https://proxy.example:443;;ftp=x").unwrap();
        assert_eq!(env.https.as_deref(), Some("https://proxy.example:443"));
        assert_eq!(env.http, None);
        assert_eq!(env.all, None);
        assert!(parse_server("").is_none());
        assert!(parse_server(";;").is_none());
    }

    #[test]
    fn bypass_becomes_a_curl_list() {
        assert_eq!(
            parse_bypass("<local>;localhost;127.*;10.*").as_deref(),
            Some("localhost,127.*,10.*,127.0.0.1,::1")
        );
        assert_eq!(
            parse_bypass("<local>").as_deref(),
            Some("localhost,127.0.0.1,::1")
        );
        assert_eq!(parse_bypass(""), None);
        assert_eq!(
            parse_bypass("a.com;A.COM").as_deref(),
            Some("a.com,localhost,127.0.0.1,::1")
        );
    }

    #[test]
    fn proxy_switch_accepts_common_false_spellings() {
        for value in ["false", "FALSE", " 0 ", "no", "off"] {
            assert!(is_disabled_value(value), "{value}");
        }
        for value in ["", "true", "1", "yes", "on", "banana"] {
            assert!(!is_disabled_value(value), "{value}");
        }
    }

    #[test]
    fn direct_mode_beats_every_proxy() {
        let mut command = Command::new("curl");
        apply_mode(&Mode::Direct, &mut command);

        let args: Vec<_> = command
            .get_args()
            .map(|arg| arg.to_string_lossy().into_owned())
            .collect();
        assert_eq!(args, vec!["--noproxy".to_string(), "*".to_string()]);
    }

    #[test]
    fn proxy_mode_overrides_through_the_command_line() {
        let mode = Mode::Proxy {
            url: "http://127.0.0.1:7890".to_string(),
            no_proxy: Some("localhost".to_string()),
        };
        let mut command = Command::new("curl");
        apply_mode(&mode, &mut command);

        let args: Vec<_> = command
            .get_args()
            .map(|arg| arg.to_string_lossy().into_owned())
            .collect();
        assert_eq!(
            args,
            vec![
                "--proxy".to_string(),
                "http://127.0.0.1:7890".to_string(),
                "--noproxy".to_string(),
                "localhost".to_string(),
            ]
        );
    }
}
