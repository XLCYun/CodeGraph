/// 编译期绑定 TypeScript/JavaScript 的随包 Node 入口。
fn main() {
    std::process::exit(codegraph_launchers::main(
        codegraph_launchers::Language::TypeScriptJavaScript,
    ));
}
