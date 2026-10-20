use crate::{projection, protocol::Request, test_support as support};
use serde_json::{Value, json};
use std::{collections::BTreeMap, fs, path::Path};

/// 只在隔离目录写入声明的项目材料，所有分析之后都会检查目录没有变化。
fn write(root: &Path, path: &str, source: &str) {
    let path = root.join(path);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, source).unwrap();
}

/// 构造一个请求；None 真正省略字段，不能借 null 测试另一条宽松入口。
fn request(root: &Path, files: Option<&[&str]>, include_tests: bool) -> Request {
    let mut value = json!({
        "protocol":"codegraph.engine/1","type":"analyze","id":"analysis-1",
        "input":{"repository_root":root.canonicalize().unwrap(),"include_tests":include_tests,"profile":"rust-repository-v1"},
        "environment":{"scratch_root":std::env::temp_dir(),"platforms":[]},
        "limits":{"analysis_timeout_ms":30000,"max_result_bytes":10000000,"max_diagnostic_bytes":65536}
    });
    if let Some(files) = files {
        value["input"]["files"] = json!(files);
    }
    serde_json::from_value(value).unwrap()
}

/// 以 coverage 而非有无节点断言完整选集，空文件也必须得到明确记录。
fn coverage(result: &Value) -> BTreeMap<&str, &str> {
    result["evidence"]["files"]
        .as_array()
        .unwrap()
        .iter()
        .map(|file| {
            (
                file["path"].as_str().unwrap(),
                file["disposition"].as_str().unwrap(),
            )
        })
        .collect()
}

/// 同仓含生产成员、自定义测试目标、非成员依赖、嵌套模块和无关上下文。
fn workspace() -> tempfile::TempDir {
    let root = tempfile::tempdir().unwrap();
    let r = root.path();
    write(
        r,
        "Cargo.toml",
        "[workspace]\nmembers=['app','member']\nexclude=['dependency']\nresolver='2'\n",
    );
    write(
        r,
        "app/Cargo.toml",
        r#"
[package]
name = "app"
version = "0.1.0"
edition = "2021"
[dependencies]
dependency = { path = "../dependency" }
[[test]]
name = "custom"
path = "checks/custom.rs"
"#,
    );
    write(
        r,
        "app/src/lib.rs",
        r#"
mod nested;
#[path = "renamed.md"] mod renamed;
#[cfg(test)] #[path = "specs.rs"] mod tests;
#[cfg(any())] mod inactive;
pub fn run() { nested::call(); dependency::external(); }
pub fn block() { #[path = "block.rs"] mod local; local::call(); }
pub const RESOURCE: &[u8] = include_bytes!("payload.rs");
"#,
    );
    write(
        r,
        "app/src/nested.rs",
        "pub mod deeper; pub fn call() { deeper::deep(); }",
    );
    write(r, "app/src/nested/deeper.rs", "pub fn deep() {}");
    write(r, "app/src/renamed.md", "pub fn renamed() {}");
    write(r, "app/src/block.rs", "pub fn call() {}");
    write(r, "app/src/specs.rs", "pub fn test_support() {}");
    write(r, "app/src/inactive.rs", "pub fn inactive_file() {}");
    write(r, "app/src/unrelated.rs", "pub fn unrelated() {}");
    fs::write(r.join("app/src/payload.rs"), [0xff, 0xfe]).unwrap();
    write(r, "app/src/bin/tool.rs", "fn main() {}");
    write(
        r,
        "app/checks/custom.rs",
        "mod support; pub fn custom_test_support() {}",
    );
    write(r, "app/checks/support.rs", "pub fn support() {}");
    write(
        r,
        "app/build.rs",
        "fn main() { std::fs::write(\"executed\", \"forbidden\").unwrap(); }",
    );
    write(
        r,
        "member/Cargo.toml",
        "[package]\nname='member'\nversion='0.1.0'\nedition='2021'\n",
    );
    write(r, "member/src/lib.rs", "");
    write(
        r,
        "dependency/Cargo.toml",
        "[package]\nname='dependency'\nversion='0.1.0'\nedition='2021'\n",
    );
    write(r, "dependency/src/lib.rs", "pub fn external() {}");
    root
}

/// 工作区成员、声明目标、递归与块内模块都来自加载快照，VFS 资源不扩大选集。
#[test]
fn discovery_selects_members_targets_and_actual_nested_modules() {
    let root = workspace();
    let before = support::snapshot(root.path());
    let result = projection::analyze(&request(root.path(), None, true)).unwrap();
    let files = coverage(&result);
    assert_eq!(
        files.keys().copied().collect::<Vec<_>>(),
        vec![
            "app/build.rs",
            "app/checks/custom.rs",
            "app/checks/support.rs",
            "app/src/bin/tool.rs",
            "app/src/block.rs",
            "app/src/lib.rs",
            "app/src/nested.rs",
            "app/src/nested/deeper.rs",
            "app/src/renamed.md",
            "app/src/specs.rs",
            "member/src/lib.rs",
        ]
    );
    assert!(files.values().all(|&disposition| disposition == "analyzed"));
    assert!(
        result["graph"]["nodes"]
            .as_array()
            .unwrap()
            .iter()
            .all(|node| {
                !["unrelated", "inactive_file", "external"]
                    .iter()
                    .any(|name| node["name"] == *name)
            })
    );
    support::assert_unchanged(root.path(), &before);
}

/// 测试开关保留所有发现 coverage，自定义测试根与测试模块只改变 disposition 和节点。
#[test]
fn discovery_tracks_test_targets_and_keeps_explicit_selection_exact() {
    let root = workspace();
    let before = support::snapshot(root.path());
    let result = projection::analyze(&request(root.path(), None, false)).unwrap();
    let files = coverage(&result);
    for path in [
        "app/checks/custom.rs",
        "app/checks/support.rs",
        "app/src/specs.rs",
    ] {
        assert_eq!(files[path], "excluded_test", "{path}");
    }
    for path in ["app/src/lib.rs", "app/src/nested.rs", "member/src/lib.rs"] {
        assert_eq!(files[path], "analyzed", "{path}");
    }
    assert!(
        !result["graph"]["nodes"]
            .as_array()
            .unwrap()
            .iter()
            .any(|node| {
                ["test_support", "custom_test_support", "support"]
                    .iter()
                    .any(|name| node["name"] == *name)
            })
    );
    let explicit =
        projection::analyze(&request(root.path(), Some(&["app/src/lib.rs"]), true)).unwrap();
    assert_eq!(
        coverage(&explicit).keys().copied().collect::<Vec<_>>(),
        ["app/src/lib.rs"]
    );
    assert!(
        explicit["graph"]["external_symbols"]
            .as_array()
            .unwrap()
            .iter()
            .any(|symbol| { symbol["symbol"].as_str().unwrap().contains("nested") })
    );
    let explicit_test = projection::analyze(&request(
        root.path(),
        Some(&["app/checks/custom.rs"]),
        false,
    ))
    .unwrap();
    assert!(
        explicit_test["graph"]["nodes"]
            .as_array()
            .unwrap()
            .iter()
            .any(|node| node["name"] == "custom_test_support")
    );
    support::assert_unchanged(root.path(), &before);
}

/// 同一文件既是测试目标又被生产模块引用时，生产归属阻止整份源码被测试开关排除。
#[test]
fn discovery_preserves_production_use_of_a_test_target_source() {
    let root = tempfile::tempdir().unwrap();
    write(
        root.path(),
        "Cargo.toml",
        "[package]\nname='shared'\nversion='0.1.0'\n[[test]]\nname='shared'\npath='checks/shared.rs'\n",
    );
    write(root.path(), "checks/shared.rs", "pub fn helper() {}");
    for test_parent in [false, true] {
        write(
            root.path(),
            "src/lib.rs",
            &format!(
                "{} #[path=\"../checks/shared.rs\"] mod shared; pub fn run() {{}}",
                if test_parent { "#[cfg(test)]" } else { "" },
            ),
        );
        let before = support::snapshot(root.path());
        let result = projection::analyze(&request(root.path(), None, false)).unwrap();
        assert_eq!(
            coverage(&result)["checks/shared.rs"],
            if test_parent {
                "excluded_test"
            } else {
                "analyzed"
            },
        );
        let helper = result["graph"]["nodes"]
            .as_array()
            .unwrap()
            .iter()
            .find(|node| node["name"] == "helper");
        if test_parent {
            assert!(helper.is_none());
            assert!(
                !result["evidence"]["origins"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .any(|origin| origin["selected_paths"] == json!(["checks/shared.rs"]))
            );
        } else {
            let helper = helper.unwrap();
            let origin = result["evidence"]["origins"]
                .as_array()
                .unwrap()
                .iter()
                .find(|origin| origin["node_id"] == helper["id"])
                .unwrap();
            assert_eq!(origin["selected_paths"], json!(["checks/shared.rs"]));
        }
        support::assert_unchanged(root.path(), &before);
    }
}

/// 现有静态项目支持成员标记与 source 范围，非成员源码仅参与引用解析。
#[test]
fn discovery_respects_static_project_membership_and_context() {
    let root = tempfile::tempdir().unwrap();
    write(
        root.path(),
        "project/root.rs",
        "mod active; pub fn run() { dependency::external(); }",
    );
    write(root.path(), "project/active.rs", "pub fn active() {}");
    write(root.path(), "project/unrelated.rs", "pub fn unrelated() {}");
    write(root.path(), "dependency/lib.rs", "pub fn external() {}");
    write(root.path(), "rust-project.json", &json!({"crates":[
        {"root_module":"project/root.rs","edition":"2021","is_workspace_member":true,"deps":[{"crate":1,"name":"dependency"}],"cfg":[],"env":{},"source":{"include_dirs":["project"],"exclude_dirs":[]}},
        {"root_module":"dependency/lib.rs","edition":"2021","is_workspace_member":false,"deps":[],"cfg":[],"env":{}}
    ]}).to_string());
    let before = support::snapshot(root.path());
    let result = projection::analyze(&request(root.path(), None, true)).unwrap();
    assert_eq!(
        coverage(&result).keys().copied().collect::<Vec<_>>(),
        ["project/active.rs", "project/root.rs"]
    );
    assert!(
        result["graph"]["external_symbols"]
            .as_array()
            .unwrap()
            .iter()
            .any(|symbol| { symbol["symbol"].as_str().unwrap().contains("dependency") })
    );
    assert!(
        !result["graph"]["nodes"]
            .as_array()
            .unwrap()
            .iter()
            .any(|node| node["name"] == "external")
    );
    support::assert_unchanged(root.path(), &before);
}

/// 缺少或损坏配置必须失败；明确空数组不读取配置，也不探测项目工具链。
#[test]
fn discovery_errors_do_not_become_empty_success() {
    let root = tempfile::tempdir().unwrap();
    write(root.path(), "unrelated.rs", "pub fn unrelated() {}");
    let before = support::snapshot(root.path());
    assert!(projection::analyze(&request(root.path(), None, true)).is_err());
    support::assert_unchanged(root.path(), &before);
    write(root.path(), "Cargo.toml", "[invalid");
    write(root.path(), "rust-toolchain.toml", "invalid toolchain");
    let before = support::snapshot(root.path());
    assert!(projection::analyze(&request(root.path(), None, true)).is_err());
    let empty = projection::analyze(&request(root.path(), Some(&[]), true)).unwrap();
    assert!(empty["graph"]["nodes"].as_array().unwrap().is_empty());
    assert!(coverage(&empty).is_empty());
    support::assert_unchanged(root.path(), &before);
}

/// 有效的空工作区确实没有成员源码，仍须返回已加载的配置证据。
#[test]
fn discovery_accepts_a_valid_configured_empty_workspace() {
    let root = tempfile::tempdir().unwrap();
    write(
        root.path(),
        "Cargo.toml",
        "[workspace]\nmembers=[]\nresolver='2'\n",
    );
    let before = support::snapshot(root.path());
    let result = projection::analyze(&request(root.path(), None, true)).unwrap();
    assert!(coverage(&result).is_empty());
    assert!(result["graph"]["nodes"].as_array().unwrap().is_empty());
    assert_eq!(
        result["evidence"]["configurations"][0]["source_paths"],
        json!(["Cargo.toml"])
    );
    support::assert_unchanged(root.path(), &before);
}

/// 配置发现与投影共用一次数据库，后续磁盘改写不能让发现改用第二个源码快照。
#[test]
fn discovery_reads_the_loaded_module_snapshot() {
    let root = tempfile::tempdir().unwrap();
    write(
        root.path(),
        "Cargo.toml",
        "[package]\nname='snapshot'\nversion='0.1.0'\n",
    );
    write(root.path(), "src/lib.rs", "mod original;");
    write(root.path(), "src/original.rs", "pub fn original() {}");
    write(root.path(), "src/replaced.rs", "pub fn replaced() {}");
    let project = crate::RustProject::load(
        &root.path().join("Cargo.toml"),
        &crate::LoadPolicy::repository_only(),
    )
    .unwrap();
    write(root.path(), "src/lib.rs", "mod replaced;");
    let repository = root.path().canonicalize().unwrap();
    let selected = project.discover_sources(&repository).unwrap();
    assert!(selected.contains(&repository.join("src/original.rs")));
    assert!(!selected.contains(&repository.join("src/replaced.rs")));
}

/// 祖先工作区中的其他成员保留为上下文；仓内清单直接声明的越界目标必须报错。
#[test]
fn discovery_distinguishes_sibling_workspace_context_from_escaping_targets() {
    let root = workspace();
    let result = projection::analyze(&request(&root.path().join("app"), None, false)).unwrap();
    assert!(coverage(&result).contains_key("src/lib.rs"));
    assert!(!coverage(&result).keys().any(|path| path.contains("member")));

    let outer = tempfile::tempdir().unwrap();
    write(
        outer.path(),
        "project/Cargo.toml",
        "[package]\nname='escape'\nversion='0.1.0'\n[lib]\npath='../outside.rs'\n",
    );
    write(outer.path(), "outside.rs", "pub fn outside() {}");
    let error =
        projection::analyze(&request(&outer.path().join("project"), None, true)).unwrap_err();
    assert!(format!("{error:#}").contains("目标根越出仓库"), "{error:#}");
}

/// 三档仅改变解析上下文，全部依赖模式也不会选中仓内非成员依赖或执行构建脚本。
#[test]
fn discovery_keeps_all_dependency_scopes_read_only_and_selected_members_equal() {
    let root = workspace();
    let toolchain = support::project_toolchain(root.path());
    let before = support::snapshot(root.path());
    let mut expected = None;
    for profile in [
        "rust-repository-v1",
        "rust-standard-library-v1",
        "rust-all-dependencies-v1",
    ] {
        let mut request = request(root.path(), None, false);
        request.input.profile = profile.into();
        if profile != "rust-repository-v1" {
            request.environment.platforms = vec![
                serde_json::from_value(json!({
                    "id":"project-rust","kind":"rust-standard-library","version":"project",
                    "root":toolchain.sysroot,
                    "metadata":{"rust_src":"lib/rustlib/src/rust/library"}
                }))
                .unwrap(),
            ];
        }
        let result = projection::analyze(&request).unwrap();
        let paths: Vec<String> = coverage(&result)
            .keys()
            .map(|path| (*path).to_owned())
            .collect();
        if let Some(expected) = &expected {
            assert_eq!(&paths, expected, "{profile}");
        } else {
            expected = Some(paths);
        }
        if profile == "rust-all-dependencies-v1" {
            assert!(
                result["graph"]["external_symbols"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .any(|symbol| { symbol["symbol"].as_str().unwrap().contains("dependency") })
            );
        }
        support::assert_unchanged(root.path(), &before);
    }
}

/// 活动模块即使在加载器里可见，真实路径逃出仓库也不能进入发现结果。
#[cfg(unix)]
#[test]
fn discovery_rejects_module_symlinks_outside_repository() {
    let root = tempfile::tempdir().unwrap();
    let outside = tempfile::tempdir().unwrap();
    write(
        root.path(),
        "Cargo.toml",
        "[package]\nname='escape'\nversion='0.1.0'\n",
    );
    write(root.path(), "src/lib.rs", "mod escape;");
    write(outside.path(), "escape.rs", "pub fn outside() {}");
    std::os::unix::fs::symlink(
        outside.path().join("escape.rs"),
        root.path().join("src/escape.rs"),
    )
    .unwrap();
    assert!(projection::analyze(&request(root.path(), None, true)).is_err());
}
