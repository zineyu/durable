//! 通用副作用库：让一类对外部世界的动作最多发生一次，崩溃后能被正确地重发或放弃。
//! 它只定义规矩：不执行副作用、不持有存储、不认识任何业务类型、不写日志，也不计时。
//! 超时归各介质层（ADR 0059）。领域内核的接线在 `durable::effect` 与 `agent` 的引擎；
//! 崩溃点枚举（`matrix`）也不在这一层。

pub mod coverage;
pub mod policy;
pub mod token;
