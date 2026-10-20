//! typescript-javascript 分析器的固定公共入口。

/// 编译时绑定语言适配器，命令行不能改变语言身份。
fn main() {
    std::process::exit(codegraph_runtime::adapters::TypeScriptJavaScript.main());
}
