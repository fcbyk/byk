//! 把已校验的声明转换为确定的来源和落点。

use std::path::Path;

use super::parse::validate_asset_name;
use super::path::{join_path, join_url, last_segment, normalize_url, split_scheme};
use crate::deps::types::{Base, Declaration, DepEntry, Origin, PlannedAsset};

// ── 计划层：URL 校验、文件名推导、落点与冲突 ────────────────────────────

/// 把条目变成绝对落点与绝对来源；冲突即报错。不检查目标是否已存在——
/// 那是执行期的事，计划要能在安装前完整打印。
///
/// 相对 `src` 在这里拼上 `base`，所以执行层拿到的永远是绝对地址：URL 交给
/// `curl`，本机路径交给 `fs::copy`。
pub(crate) fn plan_entries(
    decl: &Declaration,
    deps_dir: &Path,
    key: &str,
) -> Result<Vec<PlannedAsset>, String> {
    let files = &decl.files;
    let mut assets = Vec::with_capacity(files.len());

    for entry in files {
        let origin = resolve_src(entry, decl.base.as_ref())?;
        let name = match &entry.name {
            Some(name) => name.clone(),
            None => derive_name(entry.index, &origin)?,
        };

        let dest = if entry.extract {
            deps_dir.join(entry.to.join("/"))
        } else {
            deps_dir.join(entry.to.join("/")).join(&name)
        };

        // 标签跟着**落点**走：解压条目的落点是目录，压缩包文件名不该出现在
        // 标签里——否则日志和 `..deps status` 会暗示一个根本不存在的路径。
        let base = if entry.to.is_empty() {
            key.to_string()
        } else {
            format!("{key}/{}", entry.to.join("/"))
        };
        let label = if entry.extract {
            base
        } else {
            format!("{base}/{name}")
        };

        let clash = assets
            .iter()
            .find(|asset: &&PlannedAsset| asset.dest == dest && !(asset.archive && entry.extract));
        if let Some(clash) = clash {
            return Err(format!(
                "files[{}]: target \"{label}\" is already used by files[{}]",
                entry.index, clash.index
            ));
        }

        assets.push(PlannedAsset {
            index: entry.index,
            label,
            origin,
            dest,
            archive: entry.extract,
            exec: entry.exec,
        });
    }

    // 解压到根（没有 `to`）的落点就是专属目录本身，重装时整目录替换；
    // 和其它条目并存会把它们一起换掉，所以只允许它单独存在。
    if assets.len() > 1 {
        for asset in &assets {
            if asset.archive && asset.dest == deps_dir {
                return Err(format!(
                    "files[{}]: extracting into the dependency root must be the only entry; set \"to\" to give it a directory",
                    asset.index
                ));
            }
        }
    }

    Ok(assets)
}

/// 条目的绝对来源：绝对 `src` 只做点段归约，相对 `src` 拼上 `base`。
///
/// 拼接方式跟着 base 的形态走：URL 按 RFC 3986 归约点段，路径按词法归约。
/// 两条路都不碰文件系统。
fn resolve_src(entry: &DepEntry, base: Option<&Base>) -> Result<Origin, String> {
    if split_scheme(&entry.src).is_some() {
        return Ok(Origin::Url(normalize_url(&entry.src)));
    }

    let base = base.ok_or_else(|| {
        format!(
            "files[{}]: \"src\" is a relative path but \"$deps.base\" is not set\nHint: add \"base\" with the directory these paths are relative to, or write absolute URLs",
            entry.index
        )
    })?;
    Ok(match base {
        Base::Url(url) => Origin::Url(join_url(url, &entry.src)),
        Base::Dir(dir) => Origin::File(join_path(dir, &entry.src)),
    })
}

/// 没写 `name` 时从来源的末段推导。
fn derive_name(index: usize, origin: &Origin) -> Result<String, String> {
    let candidate = match origin {
        Origin::Url(url) => last_segment(url).unwrap_or_default().to_string(),
        Origin::File(path) => path
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_default(),
    };
    if candidate.is_empty() {
        let shown = origin.as_str();
        return Err(format!(
            "files[{index}]: cannot derive a file name from \"{shown}\"; set \"name\""
        ));
    }
    validate_asset_name(index, "name", &candidate)
        .map_err(|err| {
            err.replace(
                &format!("files[{index}]: \"name\""),
                &format!("files[{index}]: derived file name is invalid (\"{candidate}\")"),
            )
        })
        .map(|_| candidate)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::deps::Origin;
    use crate::deps::parse_deps;
    use crate::deps::types::path_to_slash;
    use serde_json::{Value, json};
    use std::path::PathBuf;

    fn parse(files: Value) -> Result<Declaration, String> {
        parse_deps(&json!({ "files": files }))
    }

    fn parse_with(base: Value, files: Value) -> Result<Declaration, String> {
        parse_deps(&json!({ "base": base, "files": files }))
    }

    fn tmp_dir() -> PathBuf {
        tempfile::tempdir().unwrap().path().join("deps").join("@@t")
    }

    #[test]
    fn relative_src_needs_a_base_only_at_plan_time() {
        let decl = parse(json!(["../hhh.js"])).unwrap();
        let err = plan_entries(&decl, &tmp_dir(), "@@t").unwrap_err();
        assert!(
            err.contains("relative path but \"$deps.base\" is not set"),
            "{err}"
        );
    }

    #[test]
    fn relative_src_joins_base_and_climbs() {
        let dir = tmp_dir();
        let decl = parse_with(
            json!("https://e.com/sdsd/asfdsg/qwe/we/"),
            json!(["../hhh.js", "bin/tool", {"src": "./cfg.json", "to": "etc"}]),
        )
        .unwrap();

        let assets = plan_entries(&decl, &dir, "@@t").unwrap();

        assert_eq!(
            assets[0].origin.as_str(),
            "https://e.com/sdsd/asfdsg/qwe/hhh.js"
        );
        assert_eq!(assets[0].dest, dir.join("hhh.js"));
        assert_eq!(
            assets[1].origin.as_str(),
            "https://e.com/sdsd/asfdsg/qwe/we/bin/tool"
        );
        assert_eq!(
            assets[2].origin.as_str(),
            "https://e.com/sdsd/asfdsg/qwe/we/cfg.json"
        );
        assert_eq!(assets[2].dest, dir.join("etc").join("cfg.json"));
    }

    #[test]
    fn relative_src_keeps_its_query_when_joined() {
        let decl = parse_with(json!("https://e.com/a/"), json!(["tool?token=x"])).unwrap();
        let assets = plan_entries(&decl, &tmp_dir(), "@@t").unwrap();
        assert_eq!(assets[0].origin.as_str(), "https://e.com/a/tool?token=x");
        assert_eq!(assets[0].label, "@@t/tool");
    }

    #[test]
    fn relative_src_joins_a_local_base_as_a_path() {
        let dir = tmp_dir();
        let base = path_to_slash(&dir);
        let decl = parse_with(json!(base), json!(["../hhh.js", "bin/tool"])).unwrap();

        let assets = plan_entries(&decl, &dir, "@@t").unwrap();

        assert_eq!(
            assets[0].origin,
            Origin::File(dir.parent().unwrap().join("hhh.js"))
        );
        assert_eq!(assets[0].label, "@@t/hhh.js");
        assert_eq!(assets[1].origin, Origin::File(dir.join("bin").join("tool")));
    }

    #[test]
    fn absolute_src_ignores_a_local_base() {
        let dir = tmp_dir();
        let decl = parse_with(json!(path_to_slash(&dir)), json!(["https://e.com/x.js"])).unwrap();
        let assets = plan_entries(&decl, &dir, "@@t").unwrap();

        assert_eq!(
            assets[0].origin,
            Origin::Url("https://e.com/x.js".to_string())
        );
    }

    #[test]
    fn plan_derives_name_from_url() {
        let dir = tmp_dir();
        let decl = parse(json!(["https://e.com/dist/my-tool-1.4.tar.gz?token=x"])).unwrap();
        let assets = plan_entries(&decl, &dir, "@@t").unwrap();
        assert_eq!(assets[0].dest, dir.join("my-tool-1.4.tar.gz"));
        assert_eq!(assets[0].label, "@@t/my-tool-1.4.tar.gz");
        assert_eq!(
            assets[0].origin.as_str(),
            "https://e.com/dist/my-tool-1.4.tar.gz?token=x"
        );
    }

    #[test]
    fn plan_normalizes_dot_segments_in_url() {
        let dir = tmp_dir();
        let decl = parse(json!(["https://e.com/dist/../rel/tool"])).unwrap();
        let assets = plan_entries(&decl, &dir, "@@t").unwrap();
        assert_eq!(assets[0].origin.as_str(), "https://e.com/rel/tool");
    }

    #[test]
    fn plan_derive_failure_mentions_name() {
        let decl = parse(json!(["https://e.com/"])).unwrap();
        let err = plan_entries(&decl, &tmp_dir(), "@@t").unwrap_err();
        assert!(err.contains("set \"name\""), "{err}");
    }

    #[test]
    fn plan_extract_dest_is_the_directory() {
        let dir = tmp_dir();
        let decl = parse(json!([{"src": "https://e.com/b.tar.gz", "to": "dist", "extract": true}]))
            .unwrap();
        let assets = plan_entries(&decl, &dir, "@@t").unwrap();
        assert_eq!(assets[0].dest, dir.join("dist"));
        assert!(assets[0].archive);
        // 标签指向目录，而不是那个只用来中转的压缩包名
        assert_eq!(assets[0].label, "@@t/dist");
    }

    #[test]
    fn plan_extract_into_root_is_labelled_by_key() {
        let decl = parse(json!([{"src": "https://e.com/b.tar.gz", "extract": true}])).unwrap();
        let assets = plan_entries(&decl, &tmp_dir(), "@@t").unwrap();
        assert_eq!(assets[0].label, "@@t");
    }

    #[test]
    fn root_extract_must_be_the_only_entry() {
        let dir = tmp_dir();
        let decl = parse(json!([
            {"src": "https://e.com/b.tar.gz", "extract": true},
            {"src": "https://e.com/cfg.json"}
        ]))
        .unwrap();

        let err = plan_entries(&decl, &dir, "@@t").unwrap_err();

        assert!(err.contains("must be the only entry"), "{err}");

        // 单独一条就合法：lansend 那种发布包形态
        let single = parse(json!([{"src": "https://e.com/b.tar.gz", "extract": true}])).unwrap();
        plan_entries(&single, &dir, "@@t").unwrap();
    }

    #[test]
    fn plan_detects_clashes() {
        let decl =
            parse(json!(["https://e.com/a", {"src": "https://e.com/b", "name": "a"}])).unwrap();
        let err = plan_entries(&decl, &tmp_dir(), "@@t").unwrap_err();
        assert!(err.contains("already used by files[0]"), "{err}");
    }

    #[test]
    fn plan_allows_file_into_extracted_directory() {
        let decl = parse(json!([
            {"src": "https://e.com/b.tar.gz", "to": "dist", "extract": true},
            {"src": "https://e.com/README", "to": "dist", "name": "README"}
        ]))
        .unwrap();
        plan_entries(&decl, &tmp_dir(), "@@t").unwrap();
    }
}
