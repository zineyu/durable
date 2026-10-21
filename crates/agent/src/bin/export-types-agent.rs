//! 把 agent 协议类型导出为 TS 定义的二进制，由 `scripts/typegen.sh` 调用。

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let directory =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../client/src/api/generated");
    agent::export_types(&directory)
}
