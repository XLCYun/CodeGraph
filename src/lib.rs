//! 各固定语言入口复用的私有运行时；不发布 Rust 库或通用二进制。
#![allow(dead_code)]
pub mod adapters;
mod child_process;
mod cli;
mod code_graph;
#[cfg(test)]
mod config_dir_fixture;
mod native_analyzer;
mod pack;
#[path = "../analyzers/common/src/selection.rs"]
mod selection;
mod strict_json;
