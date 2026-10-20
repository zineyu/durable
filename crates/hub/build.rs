use std::path::Path;

fn main() {
    let dist = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../client/dist");
    println!("cargo:rerun-if-changed=../../client/dist");
    if !dist.join("index.html").exists() {
        panic!(
            "client 构建产物缺失：先执行 client 的 npm run build（需要 client/dist/index.html）"
        );
    }
}
