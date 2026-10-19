use std::fmt;

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::id::{MessageId, RunId, SessionId, deserialize_validated};
use crate::tool::ToolCallRecord;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "lowercase")]
pub enum MessageRole {
    User,
    Assistant,
}

impl MessageRole {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::User => "user",
            Self::Assistant => "assistant",
        }
    }

    /// 解析存储中的角色。
    ///
    /// 失败：取值不是已知角色。
    pub fn parse(raw: &str) -> Result<Self, MessageRoleError> {
        match raw {
            "user" => Ok(Self::User),
            "assistant" => Ok(Self::Assistant),
            other => Err(MessageRoleError(other.to_owned())),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MessageRoleError(String);

impl fmt::Display for MessageRoleError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "未知的消息角色: {}", self.0)
    }
}

impl std::error::Error for MessageRoleError {}

/// 用户输入正文。保留原始内容，只要求去掉空白后非空。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
pub struct MessageText(String);

impl MessageText {
    /// 解析消息正文。
    ///
    /// 失败：去掉空白后为空。
    pub fn parse(raw: &str) -> Result<Self, MessageTextError> {
        if raw.trim().is_empty() {
            return Err(MessageTextError);
        }
        Ok(Self(raw.to_owned()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MessageTextError;

impl fmt::Display for MessageTextError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "消息正文不能为空")
    }
}

impl std::error::Error for MessageTextError {}

deserialize_validated!(MessageText);

/// 消息节点正文。用户消息经 [`MessageText`] 校验；assistant 消息可以为空（只请求工具时）。
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct MessageContent(String);

impl MessageContent {
    pub fn new(raw: impl Into<String>) -> Self {
        Self(raw.into())
    }

    pub fn empty() -> Self {
        Self(String::new())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// 消息树节点，同时是协议中的消息形状。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct MessageNode {
    #[ts(type = "number")]
    pub id: MessageId,
    #[ts(type = "number | null")]
    pub parent_id: Option<MessageId>,
    pub role: MessageRole,
    pub text: MessageContent,
    #[ts(type = "number | null")]
    pub run_id: Option<RunId>,
    pub tool_calls: Vec<ToolCallRecord>,
    #[ts(type = "number")]
    pub created_at: i64,
}

/// 消息节点及其所属 session。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Message {
    pub(crate) session: SessionId,
    pub(crate) node: MessageNode,
}
