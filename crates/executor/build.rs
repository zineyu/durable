//! 构建期常量：构建 id。`agent` 用同一个公式算出期望值，双方在 link 启动参数上对齐。
//! 内嵌资源与平台标签在 `crates/agent/build.rs`。

use std::env;

include!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../build/build_id.rs"
));

fn main() {
    emit_build_id();
}
