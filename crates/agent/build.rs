//! 构建期常量与检查：构建 id、本地平台标签，以及 `client/dist` 内嵌资源的缺失保护。
//! 常量消费方在 `src/link/transport`，资源托管在 `src/assets.rs`。

use std::env;
use std::path::Path;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-env-changed=TARGET");
    let version = env::var("CARGO_PKG_VERSION").unwrap_or_else(|_| "0.0.0".to_owned());
    let target = env::var("TARGET").unwrap_or_else(|_| "unknown".to_owned());
    println!("cargo:rustc-env=DURABLE_BUILD_ID={version}-{target}");
    println!(
        "cargo:rustc-env=DURABLE_BUILD_PLATFORM={}",
        platform(&target)
    );
    let dist = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../client/dist");
    println!("cargo:rerun-if-changed=../../client/dist");
    if !dist.join("index.html").exists() {
        return Err(
            "client 构建产物缺失：先执行 client 的 npm run build（需要 client/dist/index.html）"
                .into(),
        );
    }
    Ok(())
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
