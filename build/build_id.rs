// 构建 id：包版本加目标三元组。`agent` 与 `executor` 的 build 脚本共用这一份。
// 两侧必须一致，否则 link 启动时的握手会把 host 判成 `incompatible`。

fn emit_build_id() {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-env-changed=TARGET");
    let version = env::var("CARGO_PKG_VERSION").unwrap_or_else(|_| "0.0.0".to_owned());
    let target = env::var("TARGET").unwrap_or_else(|_| "unknown".to_owned());
    println!("cargo:rustc-env=DURABLE_BUILD_ID={version}-{target}");
}
