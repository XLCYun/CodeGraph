//! CodeGraph 的唯一公共入口；语言实现及 Rust 模块均为内部实现。
#![allow(dead_code)]
mod child_process;
mod cli;
mod code_graph;
#[cfg(test)]
mod config_dir_fixture;
mod native_analyzer;
mod native_languages;
mod pack;
mod strict_json;

/// 二进制是唯一公共接口；退出码和 NDJSON 共同报告结果。
fn main() {
    std::process::exit(cli::main());
}
