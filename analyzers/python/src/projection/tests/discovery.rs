use super::*;
use crate::protocol::SOURCE_REVISION;
use std::fs;

/// 真实目录和运行中的测试可执行文件提供边界，直接覆盖生产投影入口。
fn fixture(contents: &[(&str, &str)]) -> (tempfile::TempDir, tempfile::TempDir, Value) {
    let root = tempfile::tempdir().unwrap();
    let scratch = tempfile::tempdir().unwrap();
    for (name, source) in contents {
        let file = root.path().join(name);
        fs::create_dir_all(file.parent().unwrap()).unwrap();
        fs::write(file, source).unwrap();
    }
    let request = json!({"protocol":"codegraph.engine/1","type":"analyze","id":"discovery","input":{"repository_root":root.path(),"include_tests":false,"profile":"python-static-v1"},"environment":{"scratch_root":scratch.path(),"platforms":[{"id":"stdlib","kind":"python-embedded-stdlib","version":"0.0.84","root":std::env::current_exe().unwrap().parent().unwrap(),"metadata":{"source_revision":SOURCE_REVISION}}]},"limits":{"analysis_timeout_ms":30000,"max_result_bytes":32000000,"max_diagnostic_bytes":65536}});
    (root, scratch, request)
}

/// 发现结果和另一版本数据库使用同一份原文；上下文并发改变也会明确失败。
#[test]
fn discovery_and_version_projection_reuse_one_frozen_source_snapshot() {
    let (root, _scratch, _request) = fixture(&[("main.py", "def before(): pass\n")]);
    let repository = root.path().canonicalize().unwrap();
    let deadline = Instant::now() + Duration::from_secs(30);
    let discovered = discovery::load(&repository, deadline, "discovery").unwrap();
    let native = &discovered.projects[&repository];
    fs::write(root.path().join("main.py"), "def after(): pass\n").unwrap();
    assert_eq!(
        native.source(Path::new("main.py")).unwrap(),
        "def before(): pass\n"
    );
    assert_eq!(
        native
            .with_minor(12)
            .unwrap()
            .source(Path::new("main.py"))
            .unwrap(),
        "def before(): pass\n"
    );
    assert!(native.ensure_unchanged(deadline).is_err());
    assert!(
        discovery::load(
            &repository,
            Instant::now() - Duration::from_secs(1),
            "discovery"
        )
        .is_err()
    );
}

/// 配置先于选集发现；上下文、被排除源码和标准声明都不能因加载而扩选。
#[test]
fn directory_projects_keep_members_roots_stubs_and_context_separate() {
    let contents = [
        ("ty.toml", "[environment]\nroot=['root_src']\n"),
        ("root_src/main.py", "def root_member(): pass\n"),
        (
            "alpha/ty.toml",
            "[environment]\nroot=['src']\nextra-paths=['../shared']\n[src]\nexclude=['src/excluded']\n",
        ),
        (
            "alpha/src/main.py",
            "from hidden import target\ndef caller():\n    target()\n    len([])\n",
        ),
        ("alpha/src/api.pyi", "def declared() -> None: ...\n"),
        ("alpha/src/test_api.py", "def test_only(): pass\n"),
        ("alpha/src/excluded/bad.py", "def {\n"),
        ("alpha/stray.py", "def stray(): pass\n"),
        ("shared/hidden.py", "def target(): pass\n"),
        (
            "beta/pyproject.toml",
            "[project]\nrequires-python='>=3.8'\n[tool.ty.environment]\nroot=['src']\n[tool.ty.src]\ninclude=['src']\n",
        ),
        ("beta/src/main.py", "def beta(): pass\n"),
    ];
    let (root, _scratch, request) = fixture(&contents);
    let result = analyze(&serde_json::from_value(request.clone()).unwrap()).unwrap();
    let paths: BTreeSet<_> = result["evidence"]["files"]
        .as_array()
        .unwrap()
        .iter()
        .map(|file| file["path"].as_str().unwrap())
        .collect();
    assert!(paths.contains("alpha/src/main.py"));
    assert!(paths.contains("alpha/src/api.pyi"));
    assert!(paths.contains("alpha/src/test_api.py"));
    assert!(paths.contains("beta/src/main.py"));
    assert!(!paths.contains("alpha/stray.py"));
    assert!(!paths.contains("alpha/src/excluded/bad.py"));
    assert!(!paths.contains("shared/hidden.py"));
    assert!(
        result["graph"]["external_symbols"]
            .as_array()
            .unwrap()
            .iter()
            .any(|symbol| symbol["name"] == "target")
    );
    assert!(
        result["evidence"]["files"]
            .as_array()
            .unwrap()
            .iter()
            .any(|file| file["path"] == "alpha/src/test_api.py"
                && file["disposition"] == "excluded_test")
    );
    let mut exact = request;
    exact["input"]["files"] = json!(["alpha/src/main.py"]);
    let exact = analyze(&serde_json::from_value(exact).unwrap()).unwrap();
    assert_eq!(exact["evidence"]["files"].as_array().unwrap().len(), 1);
    assert!(
        exact["graph"]["external_symbols"]
            .as_array()
            .unwrap()
            .iter()
            .any(|symbol| symbol["name"] == "target")
    );
    for (name, source) in contents {
        assert_eq!(fs::read_to_string(root.path().join(name)).unwrap(), source);
    }
    assert_eq!(
        walkdir::WalkDir::new(root.path())
            .into_iter()
            .filter(|entry| entry.as_ref().unwrap().file_type().is_file())
            .count(),
        contents.len()
    );
}

/// include 使用原生匹配扩展 own roots；extra-paths 本身不提供选集资格。
#[test]
fn explicit_source_policy_can_admit_context_and_nested_projects_keep_ownership() {
    for include in [
        "",
        "[src]\ninclude=['src', 'shared']\n",
        "[src]\ninclude=[]\n",
    ] {
        let configuration =
            format!("[environment]\nroot=['src']\nextra-paths=['shared']\n{include}");
        let (_root, _scratch, request) = fixture(&[
            ("ty.toml", &configuration),
            ("src/main.py", "def member(): pass\n"),
            ("shared/context.py", "def context(): pass\n"),
            ("stray.py", "def stray(): pass\n"),
            ("child/ty.toml", "[environment]\nroot=['own']\n"),
            ("child/own/main.py", "def child(): pass\n"),
            ("child/outside.py", "def outside(): pass\n"),
        ]);
        let result = analyze(&serde_json::from_value(request).unwrap()).unwrap();
        let paths: BTreeSet<_> = result["evidence"]["files"]
            .as_array()
            .unwrap()
            .iter()
            .map(|file| file["path"].as_str().unwrap())
            .collect();
        assert_eq!(
            paths.contains("shared/context.py"),
            include.contains("'shared'")
        );
        assert_eq!(
            paths.contains("src/main.py"),
            !include.contains("include=[]")
        );
        assert!(paths.contains("child/own/main.py"));
        assert!(!paths.contains("child/outside.py"));
        assert!(!paths.contains("stray.py"));
    }
    // 过滤掉的 own roots 不能因为存在 extra-paths 而失去项目根回退。
    let (_root, _scratch, request) = fixture(&[
        (
            "ty.toml",
            "[environment]\nroot=['missing']\nextra-paths=['shared']\n",
        ),
        ("main.py", "def own(): pass\n"),
        ("shared/context.py", "def context(): pass\n"),
    ]);
    let result = analyze(&serde_json::from_value(request).unwrap()).unwrap();
    assert!(
        result["evidence"]["files"]
            .as_array()
            .unwrap()
            .iter()
            .any(|file| file["path"] == "main.py")
    );
    let (_root, _scratch, request) = fixture(&[
        ("ty.toml", "[environment]\nroot=['root_src']\n"),
        ("root_src/main.py", ""),
        ("alpha/ty.toml", "[environment]\nroot=['../shared']\n"),
        ("alpha/own/main.py", "def not_selected(): pass\n"),
        ("shared/main.py", "def admitted_by_source_policy(): pass\n"),
    ]);
    let result = analyze(&serde_json::from_value(request).unwrap()).unwrap();
    let file = result["evidence"]["files"]
        .as_array()
        .unwrap()
        .iter()
        .find(|file| file["path"] == "shared/main.py")
        .unwrap();
    let configuration = result["evidence"]["configurations"]
        .as_array()
        .unwrap()
        .iter()
        .find(|config| config["id"] == file["configuration_ids"][0])
        .unwrap();
    assert_eq!(configuration["source_paths"], json!(["alpha/ty.toml"]));
    assert!(
        !result["evidence"]["files"]
            .as_array()
            .unwrap()
            .iter()
            .any(|file| file["path"] == "alpha/own/main.py")
    );
}

/// 兄弟 own-root 不能绕过已接纳目的地的源策略、配置身份或 Python 版本。
#[test]
fn admitted_sibling_policy_owns_overlapping_roots_and_versions() {
    for source_policy in ["include=[]", "exclude=['main.py']"] {
        let configuration = format!("[src]\n{source_policy}\n");
        let (_root, _scratch, mut request) = fixture(&[
            ("ty.toml", "[environment]\nroot=['root_src']\n"),
            ("root_src/main.py", "def root_member(): pass\n"),
            ("alpha/ty.toml", "[environment]\nroot=['../shared']\n"),
            ("shared/ty.toml", &configuration),
            ("shared/main.py", "def should_stay_excluded(): pass\n"),
        ]);
        let result = analyze(&serde_json::from_value(request.clone()).unwrap()).unwrap();
        assert_eq!(result["evidence"]["files"].as_array().unwrap().len(), 1);
        assert_eq!(result["evidence"]["files"][0]["path"], "root_src/main.py");
        request["input"]["files"] = json!(["shared/main.py"]);
        let exact = analyze(&serde_json::from_value(request).unwrap()).unwrap();
        assert_eq!(
            exact["evidence"]["configurations"][0]["source_paths"],
            json!(["shared/ty.toml"])
        );
    }
    let (root, _scratch, request) = fixture(&[
        ("ty.toml", "[environment]\nroot=['root_src']\n"),
        ("root_src/main.py", "def root_member(): pass\n"),
        (
            "alpha/ty.toml",
            "[environment]\nroot=['../shared']\npython-version='3.8'\n",
        ),
        ("shared/ty.toml", "[environment]\npython-version='3.12'\n"),
        ("shared/main.py", "type OwnedAlias = int\n"),
    ]);
    let repository = root.path().canonicalize().unwrap();
    let discovered = discovery::load(
        &repository,
        Instant::now() + Duration::from_secs(30),
        "discovery",
    )
    .unwrap();
    assert_eq!(
        discovered.owners["shared/main.py"],
        repository.join("shared")
    );
    assert_eq!(
        discovered.policies[&discovered.owners["shared/main.py"]].minor,
        12
    );
    let result = analyze(&serde_json::from_value(request.clone()).unwrap()).unwrap();
    let file = result["evidence"]["files"]
        .as_array()
        .unwrap()
        .iter()
        .find(|file| file["path"] == "shared/main.py")
        .unwrap();
    let owner = result["evidence"]["configurations"]
        .as_array()
        .unwrap()
        .iter()
        .find(|config| config["id"] == file["configuration_ids"][0])
        .unwrap();
    assert_eq!(owner["source_paths"], json!(["shared/ty.toml"]));
    fs::write(
        root.path().join("alpha/ty.toml"),
        "[environment]\nroot=['../shared']\npython-version='3.12'\n",
    )
    .unwrap();
    fs::write(
        root.path().join("shared/ty.toml"),
        "[environment]\npython-version='3.8'\n",
    )
    .unwrap();
    assert!(analyze(&serde_json::from_value(request).unwrap()).is_err());
}

/// 嵌套入口使用原生排除规则；include 不阻止独立项目，控制文件名排除也可阻止入口。
#[test]
fn nested_configuration_admission_respects_parent_excludes_and_native_negation() {
    for restore_dist in [false, true] {
        let configuration = format!(
            "[environment]\nroot=['src']\nextra-paths=['control']\n[src]\ninclude=['src']\nexclude=['blocked', 'control/ty.toml'{}]\n",
            if restore_dist { ", '!dist'" } else { "" }
        );
        let (root, _scratch, mut request) = fixture(&[
            ("ty.toml", &configuration),
            (
                "src/main.py",
                "from hidden import target\ndef root_member():\n    target()\n",
            ),
            ("dist/ty.toml", ""),
            ("dist/main.py", "def generated(): pass\n"),
            ("blocked/ty.toml", "[environment]\nroot=['own']\n"),
            ("blocked/own/main.py", "def blocked(): pass\n"),
            ("control/ty.toml", "[malformed"),
            ("control/hidden.py", "def target(): pass\n"),
            (
                "admitted/ty.toml",
                "[environment]\nroot=['src']\n[src]\nexclude=['deeper']\n",
            ),
            ("admitted/src/main.py", "def admitted(): pass\n"),
            ("admitted/deeper/ty.toml", "[malformed"),
            ("admitted/deeper/main.py", "def ignored(): pass\n"),
        ]);
        let result = analyze(&serde_json::from_value(request.clone()).unwrap()).unwrap();
        let paths: BTreeSet<_> = result["evidence"]["files"]
            .as_array()
            .unwrap()
            .iter()
            .map(|file| file["path"].as_str().unwrap())
            .collect();
        assert_eq!(paths.contains("dist/main.py"), restore_dist);
        assert!(paths.contains("admitted/src/main.py"));
        assert!(
            result["graph"]["external_symbols"]
                .as_array()
                .unwrap()
                .iter()
                .any(|symbol| symbol["name"] == "target")
        );
        assert!(!paths.iter().any(|path| path.starts_with("blocked/")
            || path.starts_with("control/")
            || path.starts_with("admitted/deeper/")));
        request["input"]["files"] = json!(["blocked/own/main.py"]);
        assert_eq!(analyze(&serde_json::from_value(request.clone()).unwrap()).unwrap()["evidence"]["files"].as_array().unwrap().len(), 1);
        request["input"].as_object_mut().unwrap().remove("files");
        request["input"]["repository_root"] = json!(root.path().join("dist"));
        assert_eq!(
            analyze(&serde_json::from_value(request).unwrap()).unwrap()["evidence"]["files"][0]["path"],
            "main.py"
        );
    }
    let (_root, _scratch, request) = fixture(&[
        ("ty.toml", "[src]\nexclude=['ty.toml']\n"),
        ("main.py", "def root_member(): pass\n"),
    ]);
    assert_eq!(
        analyze(&serde_json::from_value(request).unwrap()).unwrap()["evidence"]["files"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
}

/// 默认排除和脚本过滤由固定 ty 解释；显式请求仍可逐个选择相同文件。
#[test]
fn native_exclusions_negations_and_pep723_are_preserved() {
    for config in [
        "",
        "[src]\nexclude=['!dist']\n",
        "[src]\nexclude-scripts=true\n",
    ] {
        let (_root, _scratch, mut request) = fixture(&[
            ("ty.toml", config),
            ("main.py", "def member(): pass\n"),
            ("dist/generated.py", "def generated(): pass\n"),
            (
                "script.py",
                "# /// script\n# requires-python = '>=3.12'\n# ///\ntype Alias = int\n",
            ),
        ]);
        let result = analyze(&serde_json::from_value(request.clone()).unwrap()).unwrap();
        let paths: BTreeSet<_> = result["evidence"]["files"]
            .as_array()
            .unwrap()
            .iter()
            .map(|file| file["path"].as_str().unwrap())
            .collect();
        assert_eq!(
            paths.contains("dist/generated.py"),
            config.contains("!dist")
        );
        assert_eq!(
            paths.contains("script.py"),
            !config.contains("exclude-scripts=true")
        );
        request["input"]["files"] = json!(["script.py", "dist/generated.py"]);
        let exact = analyze(&serde_json::from_value(request).unwrap()).unwrap();
        assert_eq!(exact["evidence"]["files"].as_array().unwrap().len(), 2);
    }
}

/// 不支持的源策略和坏配置必须失败；旧显式选集不启用新加的 src 校验。
#[test]
fn invalid_source_policy_is_not_an_empty_success_and_exact_mode_is_unchanged() {
    for config in [
        "[src]\ninclude=3\n",
        "[src]\ninclude=['../shared']\n",
        "[src]\nexclude=['[']\n",
        "[src]\nunknown=true\n",
        "[src]\nrespect-ignore-files=true\n",
    ] {
        let (_root, _scratch, mut request) =
            fixture(&[("ty.toml", config), ("main.py", "def member(): pass\n")]);
        assert!(
            analyze(&serde_json::from_value(request.clone()).unwrap()).is_err(),
            "{config}"
        );
        request["input"]["files"] = json!(["main.py"]);
        assert!(
            analyze(&serde_json::from_value(request).unwrap()).is_ok(),
            "{config}"
        );
    }
    let (_root, _scratch, request) = fixture(&[("ty.toml", "[src\n")]);
    assert!(analyze(&serde_json::from_value(request).unwrap()).is_err());
    let (_root, _scratch, request) = fixture(&[
        ("ty.toml", "[src]\nrespect-ignore-files=false\n"),
        (".gitignore", "main.py\n"),
        ("main.py", "def member(): pass\n"),
    ]);
    assert_eq!(
        analyze(&serde_json::from_value(request).unwrap()).unwrap()["evidence"]["files"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
}

/// 路径校验发生在测试过滤之前；发现不能把逃逸链接变成合法空图。
#[cfg(unix)]
#[test]
fn discovery_rejects_source_and_configuration_symlink_escape() {
    for name in ["test_escape.py", "ty.toml"] {
        let (root, _scratch, request) = fixture(&[]);
        let outside = tempfile::NamedTempFile::new().unwrap();
        std::os::unix::fs::symlink(outside.path(), root.path().join(name)).unwrap();
        assert!(analyze(&serde_json::from_value(request).unwrap()).is_err());
    }
}

/// 省略选集、显式空集和显式越过 src 过滤是不同操作，不能共用自动过滤结果。
#[test]
fn source_policy_does_not_change_explicit_selection_and_empty_defaults() {
    for config in ["", "[src]\ninclude=[]\n", "[src]\nexclude=['**/*.py']\n"] {
        let (_root, _scratch, mut request) =
            fixture(&[("ty.toml", config), ("main.py", "def selected(): pass\n")]);
        let discovered = analyze(&serde_json::from_value(request.clone()).unwrap()).unwrap();
        assert!(
            !discovered["evidence"]["configurations"]
                .as_array()
                .unwrap()
                .is_empty()
        );
        if !config.is_empty() {
            assert!(discovered["graph"]["nodes"].as_array().unwrap().is_empty());
        }
        request["input"]["files"] = json!(["main.py"]);
        assert!(
            !analyze(&serde_json::from_value(request.clone()).unwrap()).unwrap()["graph"]["nodes"]
                .as_array()
                .unwrap()
                .is_empty()
        );
        request["input"]["files"] = json!([]);
        assert!(
            analyze(&serde_json::from_value(request).unwrap()).unwrap()["graph"]["nodes"]
                .as_array()
                .unwrap()
                .is_empty()
        );
    }
    let (_root, _scratch, request) = fixture(&[]);
    let result = analyze(&serde_json::from_value(request).unwrap()).unwrap();
    assert!(result["graph"]["nodes"].as_array().unwrap().is_empty());
    assert!(
        !result["evidence"]["configurations"]
            .as_array()
            .unwrap()
            .is_empty()
    );
}
