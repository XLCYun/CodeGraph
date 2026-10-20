use super::*;

/// 使用真实 Host 边界校验发现结果，不预先把返回路径作为可信输入。
fn build_discovered(fixture: &Fixture, value: Value, include_tests: bool) -> Result<CodeGraph> {
    let analyzer = fixture.analyzer();
    let mut request = fixture.input();
    request.files = None;
    request.options.include_tests = include_tests;
    let checked = input::validate(&analyzer, &request)?;
    let protocol::Message::Ok(completed) = protocol::decode(&serde_json::to_vec(&value).unwrap())?
    else {
        panic!("fixture 必须为成功封套");
    };
    graph::build(completed, &checked, &analyzer.identity)
}

/// 私有请求保留省略、空数组、明确数组三种形式，不能把省略序列化为 null。
#[test]
fn selection_modes_preserve_the_private_wire_contract() {
    let fixture = Fixture::new();
    let analyzer = fixture.analyzer();
    let mut request = fixture.input();
    for files in [None, Some(Vec::new()), request.files.clone()] {
        request.files = files.clone();
        let checked = input::validate(&analyzer, &request).unwrap();
        let wire = checked.request(&analyzer, &fixture.tools).unwrap();
        match files {
            None => assert!(wire["input"].get("files").is_none()),
            Some(files) => {
                let mut files: Vec<_> = files.iter().map(|p| p.to_str().unwrap()).collect();
                files.sort();
                assert_eq!(wire["input"]["files"], json!(files));
            }
        }
    }
}

/// 发现 coverage 自身成为选集，普通空文件及非标准后缀仍须被覆盖。
#[test]
fn discovery_accepts_complete_coverage_without_extension_guessing() {
    let fixture = Fixture::new();
    build_discovered(&fixture, result(), false).unwrap();
    fs::write(fixture.repository.join("module.inc"), "").unwrap();
    let mut value = result();
    value["evidence"]["files"][1]["path"] = json!("module.inc");
    build_discovered(&fixture, value.clone(), false).unwrap();
    // 显式选集不能被同一份发现结果偷偷扩张。
    assert!(fixture.build(value).is_err());
}

/// 每个返回路径及来源都要通过检查，配置或图失败不能退化为成功空图。
#[test]
fn discovery_rejects_invalid_paths_coverage_and_origins() {
    let fixture = Fixture::new();
    fs::create_dir(fixture.repository.join("directory.go")).unwrap();
    for path in [
        "../escape.go",
        "/absolute.go",
        "a\\b.go",
        "C:/x.go",
        "",
        "a//b.go",
        "./empty.go",
        "missing.go",
        "directory.go",
        "中文.go",
    ] {
        let mut value = result();
        value["evidence"]["files"][1]["path"] = json!(path);
        assert!(
            build_discovered(&fixture, value, false).is_err(),
            "{path:?}"
        );
    }
    for case in 0..7 {
        let mut value = result();
        match case {
            0 => value["evidence"]["configurations"] = json!([]),
            1 => value["evidence"]["files"][0]["configuration_ids"] = json!([]),
            2 => value["evidence"]["files"][0]["configuration_ids"] = json!(["missing"]),
            3 => value["evidence"]["origins"][0]["selected_paths"] = json!(["missing.go"]),
            4 => value["evidence"]["files"][0]["disposition"] = json!("inactive"),
            5 => value["evidence"]["origins"] = json!([]),
            6 => value["evidence"]["configurations"][0]["source_paths"] = json!(["../Cargo.toml"]),
            _ => unreachable!(),
        }
        assert!(
            build_discovered(&fixture, value, false).is_err(),
            "case {case}"
        );
    }
    let mut value = result();
    value["evidence"]["files"][1]["disposition"] = json!("excluded_test");
    build_discovered(&fixture, value.clone(), false).unwrap();
    assert!(build_discovered(&fixture, value, true).is_err());
}

/// 有效配置可以发现零源码；缺配置的空结果与超过数量限额的结果都拒绝。
#[test]
fn discovery_empty_results_require_configuration_and_file_count_is_bounded() {
    let fixture = Fixture::new();
    let mut value = result();
    value["graph"] = json!({"root_contains":[],"nodes":[],"external_symbols":[]});
    value["evidence"]["files"] = json!([]);
    value["evidence"]["origins"] = json!([]);
    build_discovered(&fixture, value.clone(), false).unwrap();
    value["evidence"]["configurations"] = json!([]);
    assert!(build_discovered(&fixture, value, false).is_err());
    let mut value = result();
    value["evidence"]["files"] = json!(vec![
        value["evidence"]["files"][0].clone();
        crate::selection::MAX_FILES + 1
    ]);
    assert!(build_discovered(&fixture, value, false)
        .unwrap_err()
        .message
        .contains("数量上限"));
}

/// 真实符号链接不能把仓外文件或本语言禁止的物理别名包装成发现结果。
#[cfg(unix)]
#[test]
fn discovery_checks_canonical_containment_and_alias_policy() {
    let fixture = Fixture::new();
    std::os::unix::fs::symlink(&fixture.tools, fixture.repository.join("outside")).unwrap();
    std::os::unix::fs::symlink(
        fixture.repository.join("中文.go"),
        fixture.repository.join("alias.go"),
    )
    .unwrap();
    for path in ["outside/executable", "alias.go"] {
        let mut value = result();
        value["evidence"]["files"][1]["path"] = json!(path);
        assert!(build_discovered(&fixture, value, false).is_err(), "{path}");
    }
}

/// 配置和源码都必须可读；权限检查生效的平台不能仅靠存在性接受成功结果。
#[cfg(unix)]
#[test]
fn discovery_rejects_unreadable_source_or_configuration() {
    use std::os::unix::fs::PermissionsExt;
    let fixture = Fixture::new();
    for relative in ["empty.go", "go.mod"] {
        let path = fixture.repository.join(relative);
        let original = fs::metadata(&path).unwrap().permissions();
        fs::set_permissions(&path, fs::Permissions::from_mode(0)).unwrap();
        let readable = fs::File::open(&path).is_ok();
        let result = build_discovered(&fixture, result(), false);
        fs::set_permissions(&path, original).unwrap();
        if !readable {
            assert!(result.is_err(), "{relative}");
        }
    }
}
