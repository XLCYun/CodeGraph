//! Rust/Python 进程内复用公共 DTO、输入边界和构图检查，不承担跨语言宿主职责。
pub mod graph;
pub mod model;
pub mod observation;
pub mod one_shot;
pub mod public_protocol;
pub mod selection;
mod strict_json;
pub use public_protocol::PROTOCOL;

#[cfg(test)]
#[path = "tests/public_protocol.rs"]
mod public_protocol_tests;
