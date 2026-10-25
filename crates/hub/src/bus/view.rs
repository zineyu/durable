//! 节点视图：状态与说明的读写，变化时广播；登记与删除同时广播 `hub` 自身事实。
//! 上游探测如何决定状态在 `watch`。

use durable::{NodeMetadata, NodeName};

use super::Bus;
use crate::ApiError;
use crate::protocol::{HubStreamEvent, NodeMetrics, NodeStatus};

/// 连接中与已停用节点的状态说明。
const CONNECTING_DETAIL: &str = "正在建立上游连接";
pub(crate) const DISABLED_DETAIL: &str = "节点已停用";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NodeView {
    pub status: NodeStatus,
    pub detail: Option<String>,
}

impl NodeView {
    pub(crate) fn connecting() -> Self {
        Self {
            status: NodeStatus::Unreachable,
            detail: Some(CONNECTING_DETAIL.to_owned()),
        }
    }

    pub(crate) fn disabled() -> Self {
        Self {
            status: NodeStatus::Unreachable,
            detail: Some(DISABLED_DETAIL.to_owned()),
        }
    }
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

    /// 写入管理状态并推进视图纪元。只有视图真的变了才广播。
    ///
    /// # Errors
    ///
    /// - `ApiError::internal`：hub 的状态锁已被毒化。
    pub fn set_view(&self, name: NodeName, view: NodeView) -> Result<(), ApiError> {
        let mut state = self.state()?;
        *state.epochs.entry(name.clone()).or_default() += 1;
        if state.views.get(&name) == Some(&view) {
            return Ok(());
        }
        state.views.insert(name.clone(), view.clone());
        drop(state);
        self.announce_view(name, view)
    }

    /// 写入探测结果。纪元已过期（节点被停用、改名或重建订阅）时忽略。
    ///
    /// # Errors
    ///
    /// - `ApiError::internal`：hub 的状态锁已被毒化。
    pub fn set_probe_view(
        &self,
        name: NodeName,
        epoch: u64,
        view: NodeView,
    ) -> Result<(), ApiError> {
        let mut state = self.state()?;
        if state.epochs.get(&name).copied().unwrap_or(0) != epoch {
            return Ok(());
        }
        if state.views.get(&name) == Some(&view) {
            return Ok(());
        }
        state.views.insert(name.clone(), view.clone());
        drop(state);
        self.announce_view(name, view)
    }

    /// 推进并返回视图纪元，使既有探测写入失效。
    ///
    /// # Errors
    ///
    /// - `ApiError::internal`：hub 的状态锁已被毒化。
    pub fn invalidate_view(&self, name: &NodeName) -> Result<u64, ApiError> {
        let mut state = self.state()?;
        let epoch = state.epochs.entry(name.clone()).or_default();
        *epoch += 1;
        Ok(*epoch)
    }

    /// 视图真的变了才广播。
    fn announce_view(&self, name: NodeName, view: NodeView) -> Result<(), ApiError> {
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

    /// 记录一条事实事件的时间戳。
    ///
    /// # Errors
    ///
    /// - `ApiError::internal`：hub 的状态锁已被毒化。
    pub fn record_fact_time(&self, name: &NodeName, timestamp: i64) -> Result<(), ApiError> {
        let mut state = self.state()?;
        state.metrics.entry(name.clone()).or_default().last_event_at = Some(timestamp);
        Ok(())
    }

    /// 记录一次 health 的往返延迟。
    ///
    /// # Errors
    ///
    /// - `ApiError::internal`：hub 的状态锁已被毒化。
    pub fn record_latency(&self, name: &NodeName, latency_ms: u64) -> Result<(), ApiError> {
        let mut state = self.state()?;
        state.metrics.entry(name.clone()).or_default().latency_ms = Some(latency_ms);
        Ok(())
    }

    /// 节点的进程内计数。从未观测过的节点返回空值。
    ///
    /// # Errors
    ///
    /// - `ApiError::internal`：hub 的状态锁已被毒化。
    pub fn metrics(&self, name: &NodeName) -> Result<NodeMetrics, ApiError> {
        Ok(self.state()?.metrics.get(name).copied().unwrap_or_default())
    }

    /// 广播节点元数据变化，用于 URL 与启用标志的修改。
    ///
    /// # Errors
    ///
    /// - `ApiError::internal`：hub 的状态锁已被毒化。
    pub fn node_updated(&self, node: NodeMetadata) -> Result<(), ApiError> {
        self.publish_fact(|hub_seq| HubStreamEvent::NodeUpdated { hub_seq, node })
    }

    /// 记录节点登记并广播。
    ///
    /// # Errors
    ///
    /// - `ApiError::internal`：hub 的状态锁已被毒化。
    pub fn node_registered(&self, node: NodeMetadata) -> Result<(), ApiError> {
        self.publish_fact(|hub_seq| HubStreamEvent::NodeRegistered { hub_seq, node })
    }

    /// 清除节点视图与进程内计数，并广播删除。
    ///
    /// # Errors
    ///
    /// - `ApiError::internal`：hub 的状态锁已被毒化。
    pub fn node_removed(&self, name: NodeName) -> Result<(), ApiError> {
        let mut state = self.state()?;
        state.views.remove(&name);
        state.metrics.remove(&name);
        *state.epochs.entry(name.clone()).or_default() += 1;
        drop(state);
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

    /// 管理写入推进纪元后，旧订阅的探测结果不再覆盖视图。
    #[test]
    fn stale_probe_view_is_ignored() {
        let bus = Bus::new(16, 16);
        let laptop = NodeName::parse("laptop").expect("合法节点名");
        let epoch = bus.invalidate_view(&laptop).expect("取视图纪元");

        bus.set_view(laptop.clone(), NodeView::disabled())
            .expect("写入停用视图");
        bus.set_probe_view(
            laptop.clone(),
            epoch,
            view(NodeStatus::Unreachable, "连接节点失败"),
        )
        .expect("写入过期探测结果");

        assert_eq!(
            bus.view(&laptop).expect("读取视图").detail.as_deref(),
            Some(DISABLED_DETAIL),
            "过期探测不得覆盖停用说明"
        );
    }
}
