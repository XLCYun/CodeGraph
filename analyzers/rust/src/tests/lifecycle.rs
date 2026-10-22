//! 真实原生入口复用公共调用方生命周期验收。
#[path = "sdk_boundary.rs"]
mod sdk_boundary;
#[path = "../../../common/src/tests/lifecycle.rs"]
mod shared;
