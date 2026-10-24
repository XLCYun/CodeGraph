/// 编译期绑定 Java 的随包 JVM 入口。
fn main() {
    std::process::exit(codegraph_launchers::main(
        codegraph_launchers::Language::Java,
    ));
}
