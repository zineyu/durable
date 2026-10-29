//! 构建期常量：构建 id。`agent` 用同一个公式算出期望值，双方在 link 启动参数上对齐。
//! 内嵌资源与平台标签在 `crates/agent/build.rs`。

use std::env;

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-env-changed=TARGET");
    let version = env::var("CARGO_PKG_VERSION").unwrap_or_else(|_| "0.0.0".to_owned());
    let target = env::var("TARGET").unwrap_or_else(|_| "unknown".to_owned());
    println!("cargo:rustc-env=DURABLE_BUILD_ID={version}-{target}");
}
