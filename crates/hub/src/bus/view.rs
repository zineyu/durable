//! 节点视图：状态与说明的读写，变化时广播；登记与删除同时广播 `hub` 自身事实。
//! 上游探测如何决定状态在 `watch`。

use durable::{NodeMetadata, NodeName};

use super::Bus;
use crate::ApiError;
use crate::protocol::{HubStreamEvent, NodeStatus};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NodeView {
    pub status: NodeStatus,
    pub detail: Option<String>,
}

impl Bus {
    /// 节点视图。没有记录的节点按「尚未连接」的不可达处理。
    ///
    /// # Errors
    ///
    /// - `ApiError::internal`：hub 的状态锁已被毒化。
    pub fn view(&self, name: &NodeName) -> Result<NodeView, ApiError> {
        let state = self.state()?;
        Ok(state.views.get(name).cloned().unwrap_or(NodeView {
            status: NodeStatus::Unreachable,
            detail: Some("尚未连接".to_owned()),
        }))
    }

    /// 写入节点视图。只有视图真的变了才广播。
    ///
    /// # Errors
    ///
    /// - `ApiError::internal`：hub 的状态锁已被毒化。
    pub fn set_view(&self, name: NodeName, view: NodeView) -> Result<(), ApiError> {
        let previous = self.state()?.views.insert(name.clone(), view.clone());
        if previous.as_ref() == Some(&view) {
            return Ok(());
        }
        match view.status {
            NodeStatus::Ok => tracing::info!(node = %name, "节点可用"),
            status => tracing::warn!(
                node = %name,
                status = ?status,
                detail = view.detail.as_deref().unwrap_or(""),
                "节点不可用"
            ),
        }
        self.publish_fact(|hub_seq| HubStreamEvent::NodeStatusChanged {
            hub_seq,
            name,
            status: view.status,
            detail: view.detail,
        })
    }

    /// 记录节点登记并广播。
    ///
    /// # Errors
    ///
    /// - `ApiError::internal`：hub 的状态锁已被毒化。
    pub fn node_registered(&self, node: NodeMetadata) -> Result<(), ApiError> {
        self.publish_fact(|hub_seq| HubStreamEvent::NodeRegistered { hub_seq, node })
    }

    /// 清除节点视图并广播删除。
    ///
    /// # Errors
    ///
    /// - `ApiError::internal`：hub 的状态锁已被毒化。
    pub fn node_removed(&self, name: NodeName) -> Result<(), ApiError> {
        self.state()?.views.remove(&name);
        self.publish_fact(|hub_seq| HubStreamEvent::NodeRemoved { hub_seq, name })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use durable::NodeName;

    fn view(status: NodeStatus, detail: &str) -> NodeView {
        NodeView {
            status,
            detail: Some(detail.to_owned()),
        }
    }

    /// 状态说明变化也广播；连续相同的组合不发。
    #[test]
    fn set_view_broadcasts_detail_changes_once() {
        let bus = Bus::new(16, 16);
        let mut receiver = bus.sender.subscribe();
        let laptop = NodeName::parse("laptop").expect("合法节点名");

        bus.set_view(
            laptop.clone(),
            view(NodeStatus::Unreachable, "连接节点失败"),
        )
        .expect("写入视图");
        bus.set_view(
            laptop.clone(),
            view(NodeStatus::Unreachable, "连接节点失败"),
        )
        .expect("写入视图");
        bus.set_view(laptop.clone(), view(NodeStatus::Unreachable, "事件流结束"))
            .expect("写入视图");

        assert_eq!(
            bus.max_seq().expect("取最大序号"),
            2,
            "相同的 (status, detail) 只广播一次"
        );

        let mut details = Vec::new();
        while let Ok(HubStreamEvent::NodeStatusChanged { detail, .. }) = receiver.try_recv() {
            details.push(detail);
        }
        assert_eq!(
            details,
            vec![
                Some("连接节点失败".to_owned()),
                Some("事件流结束".to_owned())
            ]
        );
    }
}
