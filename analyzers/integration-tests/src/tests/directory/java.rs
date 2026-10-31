use super::*;
use crate::code_graph::LanguageAnalyzer;

/// 真实 JDT 发现声明根，生产测试后缀、明确测试根与图外源码上下文不会混淆。
#[test]
#[ignore = "需要真实 JDT helper 与固定目标 JDK，串行运行"]
fn source_root_discovery_preserves_exact_selection_and_test_roles() {
    let sources = [
        (
            "narrant-java.json",
            r#"{"schema":"java-static-v1","release":"17","source_roots":[{"path":"src","role":"main"},{"path":"checks","role":"test"}]}"#,
        ),
        (
            "src/Main.java",
            "class Main { int call(){return Helper.value();} }",
        ),
        (
            "src/Helper.java",
            "class Helper { static int value(){return 1;} }",
        ),
        ("src/RealTest.java", "class RealTest {}"),
        ("checks/Checks.java", "class Checks {}"),
        ("other/Stray.java", "invalid excluded source"),
    ];
    let repository = repository(&sources);
    let analyzer = analyzer("java");
    let mut input = input(repository.path());
    let graph = analyzer.analyze(&input).unwrap();
    let observed = names(&graph);
    assert!(
        observed.contains("Main") && observed.contains("Helper") && observed.contains("RealTest")
    );
    assert!(!observed.contains("Checks"));
    input.options.include_tests = true;
    assert!(names(&analyzer.analyze(&input).unwrap()).contains("Checks"));
    input.files = Some(vec!["src/Main.java".into()]);
    let graph = analyzer.analyze(&input).unwrap();
    assert!(!names(&graph).contains("Helper"));
    assert!(!graph.external_symbols().is_empty());
    input.files = Some(vec![]);
    assert!(analyzer.analyze(&input).unwrap().root().contains.is_empty());
    unchanged(repository.path(), &sources);
    let empty = super::repository(&[("pom.xml", "<project><modelVersion>4.0.0</modelVersion><groupId>fixture</groupId><artifactId>empty</artifactId><version>1</version><properties><maven.compiler.release>17</maven.compiler.release></properties></project>")]);
    assert!(analyzer
        .analyze(&super::input(empty.path()))
        .unwrap()
        .root()
        .contains
        .is_empty());
}

/// Traverse the unchanged public containment tree for semantic assertions.
fn data(graph: &CodeGraph) -> Vec<crate::code_graph::CodeNodeData> {
    let mut result = Vec::new();
    let mut pending = graph.root().contains.clone();
    while let Some(id) = pending.pop() {
        let node = graph.node(&id).unwrap().data();
        pending.extend(node.contains.iter().cloned());
        result.push(node.clone());
    }
    result.sort_by(|a, b| a.id.cmp(&b.id));
    result
}

#[test]
#[ignore = "requires installed Java package and standard SDK fixtures"]
fn configless_default_platform_preserves_source_identity() {
    let sources = [(
        "src/main/java/p/App.java",
        "package p; class App { void helper(){} void run(){helper();} }\r\n",
    )];
    let first = repository(&sources);
    let second = repository(&sources);
    let analyzer = analyzer("java");
    let graph = analyzer.analyze(&input(first.path())).unwrap();
    assert_eq!(
        data(&graph),
        data(&analyzer.analyze(&input(second.path())).unwrap())
    );
    let nodes = data(&graph);
    let run = nodes
        .iter()
        .find(|n| n.name.as_deref() == Some("run"))
        .unwrap();
    assert_eq!(run.source.as_deref(), Some("void run(){helper();}"));
    assert_eq!(run.extensions["java.platforms"][0]["target"], "unknown");
    assert!(run.references.iter().any(|r| r.semantic == "code.calls"
        && matches!(r.target, crate::code_graph::ReferenceTarget::Node(_))));
    unchanged(first.path(), &sources);
    unchanged(second.path(), &sources);
}

#[test]
#[ignore = "requires installed Java package and standard SDK fixtures"]
fn invalid_model_explicit_selection_keeps_source_graph() {
    let sources = [
        ("pom.xml", "<project>broken"),
        ("A.java", "class A { void run(){} }"),
    ];
    let repository = repository(&sources);
    let analyzer = analyzer("java");
    let mut request = input(repository.path());
    assert!(analyzer.analyze(&request).is_err());
    request.files = Some(vec!["A.java".into()]);
    let graph = analyzer.analyze(&request).unwrap();
    assert!(names(&graph).contains("run"));
    request.files = Some(vec![]);
    assert!(analyzer
        .analyze(&request)
        .unwrap()
        .root()
        .contains
        .is_empty());
    unchanged(repository.path(), &sources);
}

#[test]
#[ignore = "requires installed Java package and standard SDK fixtures"]
fn maven_reactor_dependencies_keep_exact_selection() {
    let sources = [
        ("pom.xml", "<project><modelVersion>4.0.0</modelVersion><groupId>fixture</groupId><artifactId>root</artifactId><version>1</version><packaging>pom</packaging><modules><module>lib</module><module>app</module></modules><properties><maven.compiler.release>17</maven.compiler.release></properties></project>"),
        ("lib/pom.xml", "<project><modelVersion>4.0.0</modelVersion><parent><groupId>fixture</groupId><artifactId>root</artifactId><version>1</version></parent><artifactId>lib</artifactId></project>"),
        ("app/pom.xml", "<project><modelVersion>4.0.0</modelVersion><parent><groupId>fixture</groupId><artifactId>root</artifactId><version>1</version></parent><artifactId>app</artifactId><dependencies><dependency><groupId>fixture</groupId><artifactId>lib</artifactId><version>1</version></dependency></dependencies></project>"),
        ("lib/src/main/java/p/Lib.java", "package p; public class Lib { public static int value(){return 1;} }"),
        ("app/src/main/java/p/App.java", "package p; class App { int run(){return Lib.value();} }"),
        ("other/Stray.java", "class Stray {}"),
    ];
    let repository = repository(&sources);
    let analyzer = analyzer("java");
    let mut request = input(repository.path());
    let full = analyzer.analyze(&request).unwrap();
    assert!(names(&full).contains("Lib"));
    assert!(!names(&full).contains("Stray"));
    request.files = Some(vec!["app/src/main/java/p/App.java".into()]);
    let exact = analyzer.analyze(&request).unwrap();
    assert!(!names(&exact).contains("Lib"));
    assert!(data(&exact)
        .iter()
        .any(
            |node| node.references.iter().any(|r| r.semantic == "code.calls"
                && matches!(
                    r.target,
                    crate::code_graph::ReferenceTarget::External { .. }
                ))
        ));
    unchanged(repository.path(), &sources);
}

#[test]
#[ignore = "requires installed Java package and pinned Gradle fixture"]
fn gradle_source_sets_keep_compiler_and_dependency_visibility() {
    let archive = std::env::var("CODEGRAPH_GRADLE_9_ZIP").expect("mandatory Gradle fixture");
    let normalized = archive.replace('\\', "/");
    let encoded: String = normalized
        .bytes()
        .map(|byte| {
            if byte.is_ascii_alphanumeric() || b"/-._~:".contains(&byte) {
                (byte as char).to_string()
            } else {
                format!("%{byte:02X}")
            }
        })
        .collect();
    let url = format!(
        "file://{}{}",
        if normalized.starts_with('/') { "" } else { "/" },
        encoded
    );
    let wrapper = format!("distributionUrl={url}\n");
    let sources = [
        ("gradle/wrapper/gradle-wrapper.properties", wrapper.as_str()),
        ("settings.gradle", "rootProject.name='fixture'"),
        ("build.gradle", "plugins { id 'java' }; tasks.withType(JavaCompile).configureEach { options.release=17 }; sourceSets { integrationTest { java.srcDir 'checks'; compileClasspath += sourceSets.main.output } }; tasks.named('compileIntegrationTestJava') { options.release=8 }; tasks.configureEach { doFirst { throw new GradleException('business task') } }"),
        ("src/main/java/p/Lib.java", "package p; public class Lib { public static int value(){return 1;} }"),
        ("checks/p/Check.java", "package p; class Check { int run(){return Lib.value();} }"),
    ];
    let repository = repository(&sources);
    let analyzer = analyzer("java");
    let mut request = input(repository.path());
    assert!(!names(&analyzer.analyze(&request).unwrap()).contains("Check"));
    request.options.include_tests = true;
    let graph = analyzer.analyze(&request).unwrap();
    let nodes = data(&graph);
    let run = nodes
        .iter()
        .find(|n| n.name.as_deref() == Some("run"))
        .unwrap();
    assert_eq!(
        run.extensions["java.platforms"][0]["compiler"]["release"],
        "8"
    );
    assert!(run.references.iter().any(|r| r.semantic == "code.calls"
        && matches!(r.target, crate::code_graph::ReferenceTarget::Node(_))));
    unchanged(repository.path(), &sources);
}
