use std::path::Path;

fn main() -> Result<(), Box<dyn std::error::Error>> {
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
