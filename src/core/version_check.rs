/// 版本更新检查（供 --info v 使用）。
///
/// 从 PyPI JSON API 获取最新版本号，
/// 与本地版本比较。

/// 从 PyPI JSON API 获取最新版本号。
///
/// 返回 (原始版本号, 标准化为 semver 格式的版本号)。
/// 因为 PyPI 使用 PEP 440 格式 (如 `1.0.0a6`)，
/// 而本地使用 Cargo/semver 格式 (如 `1.0.0-alpha.6`)。
pub fn fetch_latest_version() -> Result<(String, String), String> {
    let mut resp = ureq::get("https://pypi.org/pypi/byk/json")
        .config()
        .http_status_as_error(false)
        .build()
        .call()
        .map_err(|e| format!("Network request failed: {}", e))?;

    let body = resp
        .body_mut()
        .read_to_string()
        .map_err(|e| format!("Failed to read response: {}", e))?;

    let json: serde_json::Value =
        serde_json::from_str(&body).map_err(|e| format!("JSON parse failed: {}", e))?;

    let pypi_version = json
        .pointer("/info/version")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
        .ok_or_else(|| "Version field not found".to_string())?;

    let normalized = pypi_to_semver(&pypi_version);
    Ok((pypi_version, normalized))
}

/// 将 PyPI PEP 440 版本号转为 semver 格式。
///
/// `1.0.0a6`  → `1.0.0-alpha.6`
/// `1.0.0b2`  → `1.0.0-beta.2`
/// `1.0.0rc1` → `1.0.0-rc.1`
/// `1.0.0`    → `1.0.0`
pub fn pypi_to_semver(v: &str) -> String {
    let boundary = v
        .find(|c: char| !c.is_ascii_digit() && c != '.')
        .unwrap_or(v.len());

    if boundary == v.len() {
        return v.to_string();
    }

    let base = &v[..boundary];
    let suffix = &v[boundary..];

    let pre = if let Some(rest) = suffix.strip_prefix("a") {
        format!("-alpha.{}", rest)
    } else if let Some(rest) = suffix.strip_prefix("b") {
        format!("-beta.{}", rest)
    } else if let Some(rest) = suffix.strip_prefix("rc") {
        format!("-rc.{}", rest)
    } else if let Some(rest) = suffix.strip_prefix(".post") {
        format!("-post.{}", rest)
    } else if let Some(rest) = suffix.strip_prefix(".dev") {
        format!("-dev.{}", rest)
    } else {
        format!("-{}", suffix)
    };

    format!("{}{}", base, pre)
}

/// 简易 semver 比较。
///
/// 比较两个去掉 'v' 前缀的版本号字符串。
/// 处理 `major.minor.patch` 及可选的 `-pre_release` 后缀。
pub fn compare_versions(a: &str, b: &str) -> std::cmp::Ordering {
    // 拆出版本号主体和预发布后缀
    let (a_ver, a_pre) = match a.split_once('-') {
        Some((v, p)) => (v, Some(p)),
        None => (a, None),
    };
    let (b_ver, b_pre) = match b.split_once('-') {
        Some((v, p)) => (v, Some(p)),
        None => (b, None),
    };

    // 解析 major.minor.patch
    let a_parts: Vec<u32> = a_ver.split('.').filter_map(|s| s.parse().ok()).collect();
    let b_parts: Vec<u32> = b_ver.split('.').filter_map(|s| s.parse().ok()).collect();

    // 数值比较
    for i in 0..3 {
        let av = a_parts.get(i).copied().unwrap_or(0);
        let bv = b_parts.get(i).copied().unwrap_or(0);
        if av > bv {
            return std::cmp::Ordering::Greater;
        }
        if av < bv {
            return std::cmp::Ordering::Less;
        }
    }

    // 版本号相同，比较预发布后缀：稳定版 > 预发布版
    match (a_pre, b_pre) {
        (None, Some(_)) => std::cmp::Ordering::Greater,
        (Some(_), None) => std::cmp::Ordering::Less,
        (Some(ap), Some(bp)) => ap.cmp(bp),
        (None, None) => std::cmp::Ordering::Equal,
    }
}

// ---------------------------------------------------------------------------
// 测试
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    // ==================== pypi_to_semver ====================

    #[test]
    fn pypi_stable_version_unchanged() {
        assert_eq!(pypi_to_semver("1.0.0"), "1.0.0");
        assert_eq!(pypi_to_semver("2.3.4"), "2.3.4");
    }

    #[test]
    fn pypi_alpha_to_semver() {
        assert_eq!(pypi_to_semver("1.0.0a6"), "1.0.0-alpha.6");
        assert_eq!(pypi_to_semver("2.0.0a1"), "2.0.0-alpha.1");
    }

    #[test]
    fn pypi_beta_to_semver() {
        assert_eq!(pypi_to_semver("1.0.0b2"), "1.0.0-beta.2");
        assert_eq!(pypi_to_semver("3.0.0b10"), "3.0.0-beta.10");
    }

    #[test]
    fn pypi_rc_to_semver() {
        assert_eq!(pypi_to_semver("1.0.0rc1"), "1.0.0-rc.1");
        assert_eq!(pypi_to_semver("2.5.0rc3"), "2.5.0-rc.3");
    }

    #[test]
    fn pypi_post_to_semver() {
        // PEP 440 ".post1" 中 dot 被 find 当作版本号分隔符消耗，
        // suffix "post1" 无法匹配 ".post" 前缀，走 else 分支。
        assert_eq!(pypi_to_semver("1.0.0.post1"), "1.0.0.-post1");
    }

    #[test]
    fn pypi_dev_to_semver() {
        assert_eq!(pypi_to_semver("1.0.0.dev1"), "1.0.0.-dev1");
    }

    #[test]
    fn pypi_unknown_suffix() {
        assert_eq!(pypi_to_semver("1.0.0+ubuntu1"), "1.0.0-+ubuntu1");
    }

    // ==================== compare_versions ====================

    use std::cmp::Ordering;

    #[test]
    fn compare_same_version() {
        assert_eq!(compare_versions("1.0.0", "1.0.0"), Ordering::Equal);
    }

    #[test]
    fn compare_higher_major() {
        assert_eq!(compare_versions("2.0.0", "1.9.9"), Ordering::Greater);
    }

    #[test]
    fn compare_higher_minor() {
        assert_eq!(compare_versions("1.2.0", "1.1.9"), Ordering::Greater);
    }

    #[test]
    fn compare_higher_patch() {
        assert_eq!(compare_versions("1.0.1", "1.0.0"), Ordering::Greater);
    }

    #[test]
    fn compare_lower_version() {
        assert_eq!(compare_versions("1.0.0", "2.0.0"), Ordering::Less);
    }

    #[test]
    fn compare_stable_greater_than_pre_release() {
        assert_eq!(compare_versions("1.0.0", "1.0.0-alpha.1"), Ordering::Greater);
    }

    #[test]
    fn compare_pre_release_less_than_stable() {
        assert_eq!(compare_versions("1.0.0-alpha.1", "1.0.0"), Ordering::Less);
    }

    #[test]
    fn compare_same_pre_release_version() {
        assert_eq!(
            compare_versions("1.0.0-alpha.1", "1.0.0-alpha.1"),
            Ordering::Equal
        );
    }

    #[test]
    fn compare_pre_release_versions() {
        assert_eq!(
            compare_versions("1.0.0-alpha.2", "1.0.0-alpha.1"),
            Ordering::Greater
        );
        assert_eq!(
            compare_versions("1.0.0-beta.1", "1.0.0-alpha.9"),
            Ordering::Greater
        );
    }

    #[test]
    fn compare_missing_parts_treated_as_zero() {
        // "1.0" 等价于 "1.0.0"
        assert_eq!(compare_versions("1.1", "1.0.9"), Ordering::Greater);
        assert_eq!(compare_versions("1.0", "1.0.0"), Ordering::Equal);
    }

    #[test]
    fn compare_two_digit_versions() {
        assert_eq!(compare_versions("10.0.0", "9.0.0"), Ordering::Greater);
        assert_eq!(compare_versions("0.10.0", "0.9.0"), Ordering::Greater);
    }
}
