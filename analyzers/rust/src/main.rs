//! rust 程序直接实现公共一次性协议，同进程执行固定原生语义核心。
/// 编译期绑定语言身份；退出码区分成功、可处理失败和 CLI 用法错误。
fn main() -> std::process::ExitCode {
    codegraph_rust_engine_core::runtime::isolate_environment();
    std::process::ExitCode::from(codegraph_engine_common::one_shot::run(
        "rust",
        codegraph_rust_engine_core::runtime::analyze,
    ) as u8)
}
