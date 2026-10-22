#![expect(
    clippy::expect_used,
    reason = "工具测试辅助函数用 expect 直接报告夹具与断言失败。"
)]

use std::path::PathBuf;
use std::time::{Duration, Instant};

use reqwest::StatusCode;

mod support;

use support::provider::ToolCallSpec;
use support::{Harness, Script};

struct ToolRun {
    harness: Harness,
    session: agent::SessionMetadata,
    workspace: PathBuf,
}

async fn start(scripts: Vec<Script>) -> ToolRun {
    start_with_limits(agent::ToolLimits::default(), scripts).await
}

async fn start_with_limits(limits: agent::ToolLimits, scripts: Vec<Script>) -> ToolRun {
    let harness = Harness::start_with_limits(agent::EngineConfig::default(), scripts, limits).await;
    let session = harness.session("workspace").await;
    let workspace = harness.workspace_path("workspace");
    ToolRun {
        harness,
        session,
        workspace,
    }
}

fn call(name: &str, arguments: &str) -> Script {
    Script::ToolCall {
        id: "call_1".into(),
        name: name.into(),
        arguments: arguments.into(),
    }
}

fn done() -> Script {
    Script::Text("完成".into())
}

impl ToolRun {
    fn write(&self, name: &str, content: impl AsRef<[u8]>) {
        std::fs::write(self.workspace.join(name), content).expect("写入测试文件");
    }

    fn read(&self, name: &str) -> String {
        std::fs::read_to_string(self.workspace.join(name)).expect("读取测试文件")
    }

    fn exists(&self, name: &str) -> bool {
        self.workspace.join(name).exists()
    }

    /// 提交一条消息，等 run 结束，按入树顺序读取全部工具结局。
    ///
    /// 工具结果是独立的 tool 节点，结局在其正文的 `tool_results()` 里；
    /// 调用的 `call_id` 在发起调用的 assistant 正文的 `tool_calls()` 里。
    async fn outcomes(&self) -> Vec<agent::ToolOutcome> {
        let states = self.results().await;
        let mut outcomes = Vec::new();
        for state in &states {
            if let agent::ToolResultState::Finished { outcome } = state {
                outcomes.push(outcome.clone());
            }
        }
        assert_eq!(outcomes.len(), states.len(), "工具结局已定");
        outcomes
    }

    /// 一次消息里单个工具调用的结局。
    async fn outcome(&self) -> agent::ToolOutcome {
        let mut outcomes = self.outcomes().await;
        assert_eq!(outcomes.len(), 1, "一次消息只产生一条工具结局");
        outcomes.remove(0)
    }

    /// 全部工具结果状态，按入树顺序，并校验结局与调用一一对应。
    async fn results(&self) -> Vec<agent::ToolResultState> {
        let response = self.harness.submit_message(&self.session.id, "执行").await;
        assert_eq!(response.status(), StatusCode::CREATED, "提交消息返回 201");
        let tree = self.harness.wait_idle(&self.session.id).await;
        let run = tree.runs.first().expect("消息树有 run");
        assert_eq!(
            run.status,
            agent::RunStatus::Succeeded,
            "工具结果不终止 run"
        );
        let ids: Vec<agent::MessageId> = tree.nodes.iter().map(|node| node.id).collect();
        let bodies = self.harness.bodies(&self.session.id, &ids).await;
        let mut call_ids: Vec<&str> = Vec::new();
        let mut result_ids: Vec<&str> = Vec::new();
        let mut states: Vec<agent::ToolResultState> = Vec::new();
        for body in &bodies {
            for (call_id, _, _) in body.tool_calls() {
                call_ids.push(call_id);
            }
            for (call_id, state) in body.tool_results() {
                result_ids.push(call_id);
                states.push(state.clone());
            }
        }
        assert_eq!(call_ids, result_ids, "每个工具调用都有顺序对应的结局");
        states
    }
}

fn succeeded(content: &str) -> agent::ToolOutcome {
    agent::ToolOutcome::Succeeded {
        content: content.to_owned(),
    }
}

fn failed(code: agent::ToolErrorCode, message: &str) -> agent::ToolOutcome {
    agent::ToolOutcome::Failed {
        code,
        message: message.to_owned(),
    }
}

/// 规格 user story A-26：`read_file` 输出带行号。
#[tokio::test]
async fn reads_file_with_line_numbers() {
    let run = start(vec![call("read_file", r#"{"path":"notes.txt"}"#), done()]).await;
    run.write("notes.txt", "alpha\nbeta\ngamma\n");

    assert_eq!(
        run.outcome().await,
        succeeded("1\talpha\n2\tbeta\n3\tgamma\n")
    );
}

/// 规格 user story A-26：可选起止行，区间含两端。
#[tokio::test]
async fn reads_selected_line_range() {
    let run = start(vec![
        Script::ToolCalls(vec![
            ToolCallSpec::new(
                "call_1",
                "read_file",
                r#"{"path":"notes.txt","start_line":2,"end_line":2}"#,
            ),
            ToolCallSpec::new(
                "call_2",
                "read_file",
                r#"{"path":"notes.txt","start_line":2}"#,
            ),
        ]),
        done(),
    ])
    .await;
    run.write("notes.txt", "alpha\nbeta\ngamma\n");

    let outcomes = run.outcomes().await;
    assert_eq!(outcomes[0], succeeded("2\tbeta\n"));
    assert_eq!(outcomes[1], succeeded("2\tbeta\n3\tgamma\n"));
}

/// 规格 user story A-26：区间非法是显式错误。
#[tokio::test]
async fn rejects_invalid_line_ranges() {
    for arguments in [
        r#"{"path":"notes.txt","start_line":0}"#,
        r#"{"path":"notes.txt","start_line":3,"end_line":2}"#,
        r#"{"path":"notes.txt","start_line":2,"end_line":9}"#,
    ] {
        let run = start(vec![call("read_file", arguments), done()]).await;
        run.write("notes.txt", "alpha\nbeta\ngamma\n");

        assert!(
            matches!(
                run.outcome().await,
                agent::ToolOutcome::Failed {
                    code: agent::ToolErrorCode::InvalidLineRange,
                    ..
                }
            ),
            "{arguments} 必须被拒绝"
        );
    }
}

/// 规格 user story A-26：非 UTF-8 与读取上限是不同错误。
#[tokio::test]
async fn rejects_non_utf8_and_oversized_reads() {
    let run = start(vec![call("read_file", r#"{"path":"binary.txt"}"#), done()]).await;
    run.write("binary.txt", [0xff, 0xfe, 0x00]);
    assert!(matches!(
        run.outcome().await,
        agent::ToolOutcome::Failed {
            code: agent::ToolErrorCode::NotUtf8,
            ..
        }
    ));

    let limits = agent::ToolLimits {
        read_bytes: 5,
        ..agent::ToolLimits::default()
    };
    let run = start_with_limits(
        limits,
        vec![call("read_file", r#"{"path":"large.txt"}"#), done()],
    )
    .await;
    run.write("large.txt", "0123456789");
    assert_eq!(
        run.outcome().await,
        failed(
            agent::ToolErrorCode::ReadLimitExceeded,
            "文件 10 字节，超过 5 字节读取上限"
        )
    );
}

/// 规格 user story A-27：`write_file` 覆盖写入并返回字节数。
#[tokio::test]
async fn overwrites_file_and_reports_bytes() {
    let run = start(vec![
        call("write_file", r#"{"path":"notes.txt","content":"新内容"}"#),
        done(),
    ])
    .await;
    run.write("notes.txt", "旧内容");

    assert_eq!(run.outcome().await, succeeded("写入 9 字节: notes.txt"));
    assert_eq!(run.read("notes.txt"), "新内容");
}

/// 规格 user story A-27：父目录不存在时不隐式创建目录。
#[tokio::test]
async fn rejects_write_with_missing_parent() {
    let run = start(vec![
        call(
            "write_file",
            r#"{"path":"missing/notes.txt","content":"内容"}"#,
        ),
        done(),
    ])
    .await;

    assert_eq!(
        run.outcome().await,
        failed(
            agent::ToolErrorCode::ParentNotFound,
            "父目录不存在: missing/notes.txt"
        )
    );
    assert!(!run.exists("missing"));
}

/// 规格 user story A-30：绝对路径与 `..` 逃逸都被拒绝。
#[tokio::test]
async fn rejects_absolute_and_parent_paths() {
    for path in ["/etc/passwd", "../escape.txt", "sub/../../escape.txt"] {
        let run = start(vec![
            call(
                "write_file",
                &serde_json::json!({ "path": path, "content": "逃逸" }).to_string(),
            ),
            done(),
        ])
        .await;

        assert!(
            matches!(
                run.outcome().await,
                agent::ToolOutcome::Failed {
                    code: agent::ToolErrorCode::PathEscape,
                    ..
                }
            ),
            "{path} 必须被拒绝"
        );
        assert!(!run.harness.directory.path().join("escape.txt").exists());
    }
}

/// 规格 user story A-30：符号链接目标逃逸 workspace 根时被拒绝。
#[cfg(unix)]
#[tokio::test]
async fn rejects_symlink_escape() {
    let outside = tempfile::tempdir().expect("创建外部目录");
    std::fs::write(outside.path().join("secret.txt"), "秘密").expect("写入外部文件");
    let run = start(vec![
        Script::ToolCalls(vec![
            ToolCallSpec::new("call_1", "read_file", r#"{"path":"link/secret.txt"}"#),
            ToolCallSpec::new(
                "call_2",
                "write_file",
                r#"{"path":"link/new.txt","content":"越界"}"#,
            ),
        ]),
        done(),
    ])
    .await;
    std::os::unix::fs::symlink(outside.path(), run.workspace.join("link")).expect("创建符号链接");

    let outcomes = run.outcomes().await;
    assert!(matches!(
        outcomes[0],
        agent::ToolOutcome::Failed {
            code: agent::ToolErrorCode::PathEscape,
            ..
        }
    ));
    assert!(matches!(
        outcomes[1],
        agent::ToolOutcome::Failed {
            code: agent::ToolErrorCode::PathEscape,
            ..
        }
    ));
    assert!(!outside.path().join("new.txt").exists());
}

/// 规格 user story A-28：`edit_file` 只替换唯一匹配。
#[tokio::test]
async fn edits_unique_match() {
    let run = start(vec![
        call(
            "edit_file",
            r#"{"path":"notes.txt","old_string":"beta","new_string":"gamma"}"#,
        ),
        done(),
    ])
    .await;
    run.write("notes.txt", "alpha\nbeta\n");

    assert_eq!(
        run.outcome().await,
        succeeded("替换 1 处，写入 12 字节: notes.txt")
    );
    assert_eq!(run.read("notes.txt"), "alpha\ngamma\n");
}

/// 规格 user story A-28：未找到与多处匹配是不同错误。
#[tokio::test]
async fn reports_edit_mismatch_and_missing_file() {
    let run = start(vec![
        Script::ToolCalls(vec![
            ToolCallSpec::new(
                "call_1",
                "edit_file",
                r#"{"path":"notes.txt","old_string":"absent","new_string":"x"}"#,
            ),
            ToolCallSpec::new(
                "call_2",
                "edit_file",
                r#"{"path":"duplicate.txt","old_string":"beta","new_string":"x"}"#,
            ),
            ToolCallSpec::new(
                "call_3",
                "edit_file",
                r#"{"path":"absent.txt","old_string":"beta","new_string":"x"}"#,
            ),
            ToolCallSpec::new(
                "call_4",
                "edit_file",
                r#"{"path":"notes.txt","old_string":"","new_string":"x"}"#,
            ),
        ]),
        done(),
    ])
    .await;
    run.write("notes.txt", "alpha\nbeta\n");
    run.write("duplicate.txt", "beta\nbeta\n");

    let outcomes = run.outcomes().await;
    assert_eq!(
        outcomes[0],
        failed(
            agent::ToolErrorCode::EditNotFound,
            "old_string 未匹配: notes.txt"
        )
    );
    assert_eq!(
        outcomes[1],
        failed(
            agent::ToolErrorCode::EditNotUnique,
            "old_string 匹配 2 处，必须唯一: duplicate.txt"
        )
    );
    assert_eq!(
        outcomes[2],
        failed(agent::ToolErrorCode::FileNotFound, "文件不存在: absent.txt")
    );
    assert!(matches!(
        outcomes[3],
        agent::ToolOutcome::Failed {
            code: agent::ToolErrorCode::InvalidArguments,
            ..
        }
    ));
    assert_eq!(run.read("notes.txt"), "alpha\nbeta\n");
}

/// 规格 user story A-29：bash 返回合并输出、退出码与耗时。
#[tokio::test]
async fn reports_bash_output_and_exit_code() {
    let run = start(vec![
        Script::ToolCalls(vec![
            ToolCallSpec::new("call_1", "bash", r#"{"command":"echo out; echo err >&2"}"#),
            ToolCallSpec::new("call_2", "bash", r#"{"command":"exit 3"}"#),
        ]),
        done(),
    ])
    .await;

    let outcomes = run.outcomes().await;
    let agent::ToolOutcome::Succeeded { content } = &outcomes[0] else {
        panic!("命令成功执行: {:?}", outcomes[0]);
    };
    assert!(content.starts_with("退出码: 0\n耗时: "), "{content}");
    assert!(content.ends_with("输出:\nout\nerr\n"), "{content}");
    let agent::ToolOutcome::Succeeded { content } = &outcomes[1] else {
        panic!("非零退出不是工具错误: {:?}", outcomes[1]);
    };
    assert!(content.starts_with("退出码: 3\n"), "{content}");
}

/// 规格 user story A-29：bash 的 cwd 限定 workspace 根内。
#[tokio::test]
async fn runs_bash_in_requested_cwd() {
    let run = start(vec![
        call("bash", r#"{"command":"cat marker.txt","cwd":"sub"}"#),
        done(),
    ])
    .await;
    std::fs::create_dir(run.workspace.join("sub")).expect("创建子目录");
    run.write("sub/marker.txt", "子目录内容");

    let agent::ToolOutcome::Succeeded { content } = run.outcome().await else {
        panic!("命令在子目录执行");
    };
    assert!(content.ends_with("输出:\n子目录内容"), "{content}");
}

/// 规格 user story A-29：cwd 越出 workspace 根时被拒绝。
#[tokio::test]
async fn rejects_bash_cwd_outside_workspace() {
    let run = start(vec![
        call("bash", r#"{"command":"pwd","cwd":"../.."}"#),
        done(),
    ])
    .await;

    assert!(
        matches!(
            run.outcome().await,
            agent::ToolOutcome::Failed {
                code: agent::ToolErrorCode::PathEscape,
                ..
            }
        ),
        "cwd 越出根必须被拒绝"
    );
}

/// 规格 user story A-29：输出超过上限时标注截断。
#[tokio::test]
async fn truncates_bash_output() {
    let limits = agent::ToolLimits {
        bash_output_bytes: 8,
        ..agent::ToolLimits::default()
    };
    let run = start_with_limits(
        limits,
        vec![
            call("bash", r#"{"command":"printf 0123456789abcdef"}"#),
            done(),
        ],
    )
    .await;

    let agent::ToolOutcome::Succeeded { content } = run.outcome().await else {
        panic!("截断不是错误");
    };
    assert!(
        content.contains("输出已截断：保留前 8 字节，共 16 字节\n"),
        "{content}"
    );
    assert!(content.ends_with("输出:\n01234567"), "{content}");
}

/// 规格 user story A-29：超时杀掉整个进程组，后台子进程不再产生输出。
#[tokio::test]
async fn times_out_bash_and_kills_process_group() {
    let limits = agent::ToolLimits {
        bash_timeout: Duration::from_millis(300),
        ..agent::ToolLimits::default()
    };
    let run = start_with_limits(
        limits,
        vec![
            call(
                "bash",
                r#"{"command":"(sleep 2; echo done > marker.txt) & sleep 5"}"#,
            ),
            done(),
        ],
    )
    .await;

    let started = Instant::now();
    let outcome = run.outcome().await;
    assert!(
        started.elapsed() < Duration::from_secs(4),
        "超时必须中止命令"
    );
    assert_eq!(
        outcome,
        failed(agent::ToolErrorCode::BashTimeout, "命令超过 300 ms 未结束")
    );
    tokio::time::sleep(Duration::from_millis(2500)).await;
    assert!(!run.exists("marker.txt"), "被杀进程组不再产生输出");
}

/// 规格 user story A-10 与 A-29：取消 run 杀掉在途命令的进程组。
#[cfg(unix)]
#[tokio::test]
async fn cancels_bash_process_group() {
    let run = start(vec![
        call(
            "bash",
            r#"{"command":"(sleep 2; echo done > marker.txt) & echo $$ > pid.txt; sleep 5"}"#,
        ),
        done(),
    ])
    .await;
    assert_eq!(
        run.harness
            .submit_message(&run.session.id, "长时间命令")
            .await
            .status(),
        StatusCode::CREATED
    );

    let pid_path = run.workspace.join("pid.txt");
    let mut pid = String::new();
    for _ in 0..250 {
        if let Ok(text) = std::fs::read_to_string(&pid_path) {
            pid = text.trim().to_owned();
            if !pid.is_empty() {
                break;
            }
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    assert!(!pid.is_empty(), "命令已启动并记录 pid");

    assert_eq!(
        run.harness.cancel_run(&run.session.id).await.status(),
        StatusCode::NO_CONTENT
    );
    let tree = run.harness.wait_idle(&run.session.id).await;
    assert_eq!(tree.runs[0].status, agent::RunStatus::Cancelled);

    let alive = std::process::Command::new("kill")
        .args(["-0", &pid])
        .stderr(std::process::Stdio::null())
        .status()
        .expect("执行 kill -0");
    assert!(!alive.success(), "被取消的命令进程已结束");
    tokio::time::sleep(Duration::from_millis(2500)).await;
    assert!(!run.exists("marker.txt"), "后台子进程随进程组被终止");
}

/// 规格「工具」：未知工具与参数错误都是结构化失败。
#[tokio::test]
async fn reports_unknown_tool_and_invalid_arguments() {
    let run = start(vec![
        Script::ToolCalls(vec![
            ToolCallSpec::new("call_1", "list_dir", "{}"),
            ToolCallSpec::new("call_2", "bash", "{}"),
        ]),
        done(),
    ])
    .await;

    let outcomes = run.outcomes().await;
    assert_eq!(
        outcomes[0],
        failed(agent::ToolErrorCode::UnknownTool, "未知工具: list_dir")
    );
    assert!(matches!(
        outcomes[1],
        agent::ToolOutcome::Failed {
            code: agent::ToolErrorCode::InvalidArguments,
            ..
        }
    ));
}
