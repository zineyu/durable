//! hub 集成测试的共享辅助：进程内 hub 与 agent 的启动、上游假件与事件流客户端。
//!
//! 每个集成测试目标是独立 crate，会单独编译本模块，因此子模块各自声明未使用条目的例外。

pub(crate) mod fixtures;
pub(crate) mod harness;
