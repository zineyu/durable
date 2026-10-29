//! 构建期常量：构建 id 与本地平台标签。
//! 消费方在 `src/link/transport`：远端文件名、载荷选择与 `incompatible` 判定都据它们。

use std::env;

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-env-changed=TARGET");
    let version = env::var("CARGO_PKG_VERSION").unwrap_or_else(|_| "0.0.0".to_owned());
    let target = env::var("TARGET").unwrap_or_else(|_| "unknown".to_owned());
    println!("cargo:rustc-env=DURABLE_BUILD_ID={version}-{target}");
    println!(
        "cargo:rustc-env=DURABLE_BUILD_PLATFORM={}",
        platform(&target)
    );
}

/// 目标三元组 → `系统-架构` 标签：`aarch64-apple-darwin` 得到 `darwin-aarch64`。
fn platform(target: &str) -> String {
    let arch = match target.split('-').next().unwrap_or_default() {
        "arm64" => "aarch64",
        other => other,
    };
    let system = if target.contains("-linux") {
        "linux"
    } else if target.contains("-darwin") {
        "darwin"
    } else if target.contains("-windows") {
        "windows"
    } else {
        "unknown"
    };
    format!("{system}-{arch}")
}
