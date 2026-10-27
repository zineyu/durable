//! 消息正文的内容块、角色与消息树节点；骨架描述结构与块状态，正文单独存放。
//! 树的问题在 `tree`，分支选择规则在 `store/branch`，快照结构在 `session`。

use std::fmt;

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::id::{MessageId, RunId, SessionId, deserialize_validated};
use crate::tool::ToolResultState;

/// 消息在树中的角色。树里没有 `system`：系统提示由请求装配层提供。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, TS)]
#[serde(rename_all = "lowercase")]
pub enum MessageRole {
    User,
    Assistant,
    Tool,
    Compaction,
}

impl MessageRole {
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::User => "user",
            Self::Assistant => "assistant",
            Self::Tool => "tool",
            Self::Compaction => "compaction",
        }
    }

    /// 解析存储中的角色。
    ///
    /// # Errors
    ///
    /// - `MessageRoleError`：取值不是已知角色。
    pub fn parse(raw: &str) -> Result<Self, MessageRoleError> {
        match raw {
            "user" => Ok(Self::User),
            "assistant" => Ok(Self::Assistant),
            "tool" => Ok(Self::Tool),
            "compaction" => Ok(Self::Compaction),
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

/// 内容块的类型标签，与 [`ContentPart`] 的变体一一对应。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
pub enum ContentKind {
    Text,
    Reasoning,
    ToolCall,
    ToolResult,
    Image,
}

impl ContentKind {
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Text => "text",
            Self::Reasoning => "reasoning",
            Self::ToolCall => "tool_call",
            Self::ToolResult => "tool_result",
            Self::Image => "image",
        }
    }

    /// 解析存储中的块类型。
    ///
    /// # Errors
    ///
    /// - `ContentKindError`：取值不是已知类型。
    pub fn parse(raw: &str) -> Result<Self, ContentKindError> {
        match raw {
            "text" => Ok(Self::Text),
            "reasoning" => Ok(Self::Reasoning),
            "tool_call" => Ok(Self::ToolCall),
            "tool_result" => Ok(Self::ToolResult),
            "image" => Ok(Self::Image),
            other => Err(ContentKindError(other.to_owned())),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContentKindError(String);

impl fmt::Display for ContentKindError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "未知的内容块类型: {}", self.0)
    }
}

impl std::error::Error for ContentKindError {}

/// 块在骨架中的状态。只有工具结果块会出现非 `complete` 的取值。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
pub enum PartState {
    Complete,
    Running,
    Unknown,
}

impl PartState {
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Complete => "complete",
            Self::Running => "running",
            Self::Unknown => "unknown",
        }
    }

    /// 解析存储中的块状态。
    ///
    /// # Errors
    ///
    /// - `PartStateError`：取值不是已知状态。
    pub fn parse(raw: &str) -> Result<Self, PartStateError> {
        match raw {
            "complete" => Ok(Self::Complete),
            "running" => Ok(Self::Running),
            "unknown" => Ok(Self::Unknown),
            other => Err(PartStateError(other.to_owned())),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PartStateError(String);

impl fmt::Display for PartStateError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "未知的块状态: {}", self.0)
    }
}

impl std::error::Error for PartStateError {}

/// provider 私有的不透明元数据：签名、thought signature、图像令牌等。
///
/// 领域不解释它的内容，也不允许单独写入；它随所属块一起存取。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct ProviderMeta(#[ts(type = "unknown")] pub serde_json::Value);

/// 一条消息正文中的一个类型化片段。块序即产生顺序。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(
    tag = "kind",
    rename_all = "snake_case",
    rename_all_fields = "camelCase"
)]
pub enum ContentPart {
    Text {
        text: String,
        provider_meta: Option<ProviderMeta>,
    },
    Reasoning {
        text: String,
        provider_meta: Option<ProviderMeta>,
    },
    ToolCall {
        call_id: String,
        name: String,
        arguments: String,
        provider_meta: Option<ProviderMeta>,
    },
    ToolResult {
        call_id: String,
        state: ToolResultState,
        provider_meta: Option<ProviderMeta>,
    },
    Image {
        media_type: String,
        data: String,
        provider_meta: Option<ProviderMeta>,
    },
}

impl ContentPart {
    #[must_use]
    pub fn text(text: impl Into<String>) -> Self {
        Self::Text {
            text: text.into(),
            provider_meta: None,
        }
    }

    #[must_use]
    pub fn reasoning(text: impl Into<String>) -> Self {
        Self::Reasoning {
            text: text.into(),
            provider_meta: None,
        }
    }

    #[must_use]
    pub fn tool_call(
        call_id: impl Into<String>,
        name: impl Into<String>,
        arguments: impl Into<String>,
    ) -> Self {
        Self::ToolCall {
            call_id: call_id.into(),
            name: name.into(),
            arguments: arguments.into(),
            provider_meta: None,
        }
    }

    #[must_use]
    pub fn tool_result(call_id: impl Into<String>, state: ToolResultState) -> Self {
        Self::ToolResult {
            call_id: call_id.into(),
            state,
            provider_meta: None,
        }
    }

    #[must_use]
    pub fn kind(&self) -> ContentKind {
        match self {
            Self::Text { .. } => ContentKind::Text,
            Self::Reasoning { .. } => ContentKind::Reasoning,
            Self::ToolCall { .. } => ContentKind::ToolCall,
            Self::ToolResult { .. } => ContentKind::ToolResult,
            Self::Image { .. } => ContentKind::Image,
        }
    }

    /// 块在骨架中的状态。
    #[must_use]
    pub fn state(&self) -> PartState {
        match self {
            Self::ToolResult {
                state: ToolResultState::Running,
                ..
            } => PartState::Running,
            Self::ToolResult {
                state: ToolResultState::Unknown,
                ..
            } => PartState::Unknown,
            _ => PartState::Complete,
        }
    }

    /// 块序列化后的字节数，与存储中的 `body` 长度一致。
    #[must_use]
    pub fn byte_size(&self) -> u32 {
        serde_json::to_vec(self).map_or(0, |bytes| u32::try_from(bytes.len()).unwrap_or(u32::MAX))
    }

    #[must_use]
    pub fn skeleton(&self, seq: u32) -> PartSkeleton {
        PartSkeleton {
            seq,
            kind: self.kind(),
            size: self.byte_size(),
            state: self.state(),
        }
    }
}

/// 用户输入正文。保留原始内容，只要求去掉空白后非空。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
pub struct MessageText(String);

impl MessageText {
    /// 解析消息正文。
    ///
    /// # Errors
    ///
    /// - `MessageTextError`：正文去掉首尾空白后为空。
    pub fn parse(raw: &str) -> Result<Self, MessageTextError> {
        if raw.trim().is_empty() {
            return Err(MessageTextError);
        }
        Ok(Self(raw.to_owned()))
    }

    #[must_use]
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

/// 消息树节点：一条消息及其全部内容块。assistant 消息可以为空块数组。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MessageNode {
    pub id: MessageId,
    pub parent_id: Option<MessageId>,
    pub role: MessageRole,
    /// 该消息是流式中断后入树的部分输出，不是模型写完的完整回答。
    pub interrupted: bool,
    pub parts: Vec<ContentPart>,
    pub run_id: Option<RunId>,
    pub created_at: i64,
}

impl MessageNode {
    /// 节点骨架：不含块正文。
    #[must_use]
    pub fn skeleton(&self) -> MessageSkeleton {
        MessageSkeleton {
            id: self.id,
            parent_id: self.parent_id,
            role: self.role,
            interrupted: self.interrupted,
            run_id: self.run_id,
            created_at: self.created_at,
            parts: self
                .parts
                .iter()
                .enumerate()
                .map(|(seq, part)| part.skeleton(seq_to_u32(seq)))
                .collect(),
        }
    }

    /// 全部文本块拼接后的正文。没有文本块时为空串。
    #[must_use]
    pub fn text(&self) -> String {
        text_of(&self.parts)
    }

    /// 本消息请求的工具调用块。
    pub fn tool_calls(&self) -> impl Iterator<Item = (&str, &str, &str)> {
        tool_calls_of(&self.parts)
    }

    /// 本消息的工具结果块。
    pub fn tool_results(&self) -> impl Iterator<Item = (&str, &ToolResultState)> {
        tool_results_of(&self.parts)
    }
}

/// 消息骨架：事件与快照携带的结构描述，不含块正文。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct MessageSkeleton {
    #[ts(type = "number")]
    pub id: MessageId,
    #[ts(type = "number | null")]
    pub parent_id: Option<MessageId>,
    pub role: MessageRole,
    /// 该消息是流式中断后入树的部分输出。迁移 0010 之前的事件行与载荷没有这个字段。
    #[serde(default)]
    pub interrupted: bool,
    #[ts(type = "number | null")]
    pub run_id: Option<RunId>,
    #[ts(type = "number")]
    pub created_at: i64,
    pub parts: Vec<PartSkeleton>,
}

impl MessageSkeleton {
    /// 用读出的块正文组装完整节点。骨架里的块描述只说明形状，正文单独存放。
    #[must_use]
    pub fn into_node(self, parts: Vec<ContentPart>) -> MessageNode {
        MessageNode {
            id: self.id,
            parent_id: self.parent_id,
            role: self.role,
            interrupted: self.interrupted,
            parts,
            run_id: self.run_id,
            created_at: self.created_at,
        }
    }
}

/// 块骨架：序号、类型、字节数与状态。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct PartSkeleton {
    #[ts(type = "number")]
    pub seq: u32,
    pub kind: ContentKind,
    #[ts(type = "number")]
    pub size: u32,
    pub state: PartState,
}

/// 按消息 id 批量拉取的块正文。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct MessageBody {
    #[ts(type = "number")]
    pub id: MessageId,
    pub parts: Vec<ContentPart>,
}

impl MessageBody {
    /// 全部文本块拼接后的正文。没有文本块时为空串。
    #[must_use]
    pub fn text(&self) -> String {
        text_of(&self.parts)
    }

    /// 本消息请求的工具调用块。
    pub fn tool_calls(&self) -> impl Iterator<Item = (&str, &str, &str)> {
        tool_calls_of(&self.parts)
    }

    /// 本消息的工具结果块。
    pub fn tool_results(&self) -> impl Iterator<Item = (&str, &ToolResultState)> {
        tool_results_of(&self.parts)
    }
}

fn text_of(parts: &[ContentPart]) -> String {
    parts
        .iter()
        .filter_map(|part| match part {
            ContentPart::Text { text, .. } => Some(text.as_str()),
            _ => None,
        })
        .collect()
}

fn tool_calls_of(parts: &[ContentPart]) -> impl Iterator<Item = (&str, &str, &str)> {
    parts.iter().filter_map(|part| match part {
        ContentPart::ToolCall {
            call_id,
            name,
            arguments,
            ..
        } => Some((call_id.as_str(), name.as_str(), arguments.as_str())),
        _ => None,
    })
}

fn tool_results_of(parts: &[ContentPart]) -> impl Iterator<Item = (&str, &ToolResultState)> {
    parts.iter().filter_map(|part| match part {
        ContentPart::ToolResult { call_id, state, .. } => Some((call_id.as_str(), state)),
        _ => None,
    })
}

/// 消息节点及其所属 session。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Message {
    pub(crate) session: SessionId,
    pub(crate) node: MessageNode,
}

/// 块序号由枚举下标得出，超出 `u32` 的块数不可能出现。
fn seq_to_u32(seq: usize) -> u32 {
    u32::try_from(seq).unwrap_or(u32::MAX)
}
