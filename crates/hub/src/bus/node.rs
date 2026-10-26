//! 节点条目：一个节点的视图、进程内计数与视图纪元，以及它们的写入规则。
//! 节点表变更与状态变化的广播也在此。上游探测如何决定状态在 `watch`，广播序号与回放窗口在 `window`。

use durable::{NodeMetadata, NodeName};

use super::Bus;
use crate::ApiError;
use crate::protocol::{HubStreamEvent, NodeMetrics, NodeStatus};

/// 连接中与已停用节点的状态说明。
const CONNECTING_DETAIL: &str = "正在建立上游连接";
pub(crate) const DISABLED_DETAIL: &str = "节点已停用";
/// 总线里还没有该节点记录时的状态说明。
const ABSENT_DETAIL: &str = "尚未连接";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NodeView {
    pub status: NodeStatus,
    pub detail: Option<String>,
}

impl NodeView {
    pub(crate) fn connecting() -> Self {
        Self::unreachable(CONNECTING_DETAIL)
    }

    pub(crate) fn disabled() -> Self {
        Self::unreachable(DISABLED_DETAIL)
    }

    fn absent() -> Self {
        Self::unreachable(ABSENT_DETAIL)
    }

    fn unreachable(detail: &str) -> Self {
        Self {
            status: NodeStatus::Unreachable,
            detail: Some(detail.to_owned()),
        }
    }
}

/// 视图纪元的唯一来源：单调递增，跨条目与跨删除都不重计。
///
/// 单一序列保证任何两个条目的纪元不同，所以删除前的订阅写出的探测结果
/// 永远不会恰好命中重新登记的条目。
#[derive(Debug, Default)]
pub(crate) struct EpochClock(u64);

impl EpochClock {
    fn next(&mut self) -> u64 {
        self.0 += 1;
        self.0
    }
}

/// 总线为单个节点保存的唯一记录。视图、计数与纪元同生同灭。
///
/// 管理写入建立条目；探测写入与计数写入只更新已有条目。
#[derive(Debug, Clone)]
pub(crate) struct NodeEntry {
    pub(crate) view: NodeView,
    pub(crate) metrics: NodeMetrics,
    /// 视图纪元。取自 `EpochClock`，过期的探测写入被忽略。
    epoch: u64,
}

impl NodeEntry {
    fn absent() -> Self {
        Self {
            view: NodeView::absent(),
            metrics: NodeMetrics::default(),
            epoch: 0,
        }
    }

    /// 管理写入：取新纪元并写入视图。返回视图是否变化。
    fn set_view(&mut self, epoch: u64, view: NodeView) -> bool {
        self.epoch = epoch;
        self.replace_view(view)
    }

    /// 探测写入：纪元不符时丢弃。返回视图是否变化。
    fn accept_probe(&mut self, epoch: u64, view: NodeView) -> bool {
        if self.epoch != epoch {
            return false;
        }
        self.replace_view(view)
    }

    /// 取新纪元，使既有探测写入失效。
    fn begin_subscription(&mut self, epoch: u64) {
        self.epoch = epoch;
    }

    fn replace_view(&mut self, view: NodeView) -> bool {
        if self.view == view {
            return false;
        }
        self.view = view;
        true
    }
}

impl Bus {
    /// 节点条目。总线没有该节点时返回「尚未连接」的默认条目。
    ///
    /// # Errors
    ///
    /// - `ApiError::internal`：hub 的状态锁已被毒化。
    pub(crate) fn node_entry(&self, name: &NodeName) -> Result<NodeEntry, ApiError> {
        Ok(self
            .state()?
            .nodes
            .get(name)
            .cloned()
            .unwrap_or_else(NodeEntry::absent))
    }

    /// 写入管理状态并取新视图纪元。没有条目时建立它。只有视图真的变了才广播。
    ///
    /// # Errors
    ///
    /// - `ApiError::internal`：hub 的状态锁已被毒化。
    pub fn set_view(&self, name: NodeName, view: NodeView) -> Result<(), ApiError> {
        let changed = {
            let mut state = self.state()?;
            let epoch = state.clock.next();
            state
                .nodes
                .entry(name.clone())
                .or_insert_with(NodeEntry::absent)
                .set_view(epoch, view.clone())
        };
        if changed {
            self.announce_view(name, view)?;
        }
        Ok(())
    }

    /// 写入探测结果。没有条目或纪元已过期（节点被停用、改名或重建订阅）时丢弃。
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
        let changed = match self.state()?.nodes.get_mut(&name) {
            Some(entry) => entry.accept_probe(epoch, view.clone()),
            None => false,
        };
        if changed {
            self.announce_view(name, view)?;
        }
        Ok(())
    }

    /// 取并返回新视图纪元，使既有探测写入失效。没有条目时建立它。
    ///
    /// # Errors
    ///
    /// - `ApiError::internal`：hub 的状态锁已被毒化。
    pub fn begin_subscription(&self, name: &NodeName) -> Result<u64, ApiError> {
        let mut state = self.state()?;
        let epoch = state.clock.next();
        state
            .nodes
            .entry(name.clone())
            .or_insert_with(NodeEntry::absent)
            .begin_subscription(epoch);
        Ok(epoch)
    }

    /// 记录一条事实事件的时间戳。没有条目时丢弃。
    ///
    /// # Errors
    ///
    /// - `ApiError::internal`：hub 的状态锁已被毒化。
    pub fn record_fact_time(&self, name: &NodeName, timestamp: i64) -> Result<(), ApiError> {
        if let Some(entry) = self.state()?.nodes.get_mut(name) {
            entry.metrics.last_event_at = Some(timestamp);
        }
        Ok(())
    }

    /// 记录一次 health 的往返延迟。没有条目时丢弃。
    ///
    /// # Errors
    ///
    /// - `ApiError::internal`：hub 的状态锁已被毒化。
    pub fn record_latency(&self, name: &NodeName, latency_ms: u64) -> Result<(), ApiError> {
        if let Some(entry) = self.state()?.nodes.get_mut(name) {
            entry.metrics.latency_ms = Some(latency_ms);
        }
        Ok(())
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

    /// 删除节点条目并广播删除。
    ///
    /// # Errors
    ///
    /// - `ApiError::internal`：hub 的状态锁已被毒化。
    pub fn node_removed(&self, name: NodeName) -> Result<(), ApiError> {
        self.state()?.nodes.remove(&name);
        self.publish_fact(|hub_seq| HubStreamEvent::NodeRemoved { hub_seq, name })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn name(raw: &str) -> NodeName {
        NodeName::parse(raw).expect("合法节点名")
    }

    fn view(status: NodeStatus, detail: &str) -> NodeView {
        NodeView {
            status,
            detail: Some(detail.to_owned()),
        }
    }

    /// 管理写入建立条目；状态说明变化也广播；连续相同的组合不发。
    #[test]
    fn set_view_broadcasts_only_actual_changes() {
        let bus = Bus::new(16, 16);
        let mut receiver = bus.sender.subscribe();
        let laptop = name("laptop");

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
        let laptop = name("laptop");
        let epoch = bus.begin_subscription(&laptop).expect("取视图纪元");

        bus.set_view(laptop.clone(), NodeView::disabled())
            .expect("写入停用视图");
        bus.set_probe_view(
            laptop.clone(),
            epoch,
            view(NodeStatus::Unreachable, "连接节点失败"),
        )
        .expect("写入过期探测结果");

        assert_eq!(
            bus.node_entry(&laptop)
                .expect("读取条目")
                .view
                .detail
                .as_deref(),
            Some(DISABLED_DETAIL),
            "过期探测不得覆盖停用说明"
        );
    }

    /// ADR 0041：删除节点后读取回到「尚未连接」，计数同时清空。
    #[test]
    fn removing_node_clears_view_and_metrics() {
        let bus = Bus::new(16, 16);
        let laptop = name("laptop");
        bus.set_view(laptop.clone(), view(NodeStatus::Ok, ""))
            .expect("写入视图");
        bus.record_fact_time(&laptop, 42).expect("记录最后事件时间");
        bus.record_latency(&laptop, 7).expect("记录延迟");

        bus.node_removed(laptop.clone()).expect("删除节点");

        let entry = bus.node_entry(&laptop).expect("读取条目");
        assert_eq!(
            entry.view.status,
            NodeStatus::Unreachable,
            "删除后状态不可达"
        );
        assert_eq!(
            entry.view.detail.as_deref(),
            Some("尚未连接"),
            "删除后说明回到尚未连接"
        );
        assert_eq!(entry.metrics, NodeMetrics::default(), "删除后计数为空");
    }

    /// ADR 0041：删除之后迟到的计数写入与过期探测写入都不复活条目。
    #[test]
    fn late_writes_do_not_resurrect_removed_node() {
        let bus = Bus::new(16, 16);
        let laptop = name("laptop");
        bus.set_view(laptop.clone(), NodeView::disabled())
            .expect("写入停用视图");
        let epoch = bus.begin_subscription(&laptop).expect("取视图纪元");
        bus.node_removed(laptop.clone()).expect("删除节点");
        let seq = bus.max_seq().expect("取最大序号");

        bus.record_fact_time(&laptop, 99).expect("记录最后事件时间");
        bus.record_latency(&laptop, 5).expect("记录延迟");
        bus.set_probe_view(laptop.clone(), epoch, view(NodeStatus::Ok, ""))
            .expect("写入过期探测结果");

        let entry = bus.node_entry(&laptop).expect("读取条目");
        assert_eq!(
            entry.view.detail.as_deref(),
            Some("尚未连接"),
            "迟到探测不得建立条目"
        );
        assert_eq!(entry.metrics, NodeMetrics::default(), "迟到计数不得复活");
        assert_eq!(bus.max_seq().expect("取最大序号"), seq, "迟到写入不得广播");
    }

    /// ADR 0041：重新登记之后，删除前订阅的迟到探测不得落到新条目上。
    ///
    /// 启动路径不对已成形的条目写管理状态，直接取纪元，所以首个纪元是 1。
    /// 若纪元从零重计，重新登记后的停用视图同样落在 1，删除前的探测就能覆盖它。
    #[test]
    fn stale_probe_from_removed_entry_is_ignored() {
        let bus = Bus::new(16, 16);
        let laptop = name("laptop");
        let epoch = bus.begin_subscription(&laptop).expect("取视图纪元");
        bus.node_removed(laptop.clone()).expect("删除节点");

        bus.set_view(laptop.clone(), NodeView::disabled())
            .expect("重新登记并停用");
        let mut receiver = bus.sender.subscribe();
        bus.set_probe_view(laptop.clone(), epoch, view(NodeStatus::Ok, ""))
            .expect("写入删除前的探测结果");

        assert_eq!(
            bus.node_entry(&laptop)
                .expect("读取条目")
                .view
                .detail
                .as_deref(),
            Some(DISABLED_DETAIL),
            "删除前订阅的探测不得覆盖重新登记后的状态"
        );
        assert!(receiver.try_recv().is_err(), "过期探测不得广播");
    }

    /// ADR 0041：探测写入不建立条目。
    #[test]
    fn probe_write_does_not_create_entry() {
        let bus = Bus::new(16, 16);
        let laptop = name("laptop");

        bus.set_probe_view(laptop.clone(), 0, view(NodeStatus::Ok, ""))
            .expect("写入探测结果");

        assert_eq!(
            bus.node_entry(&laptop)
                .expect("读取条目")
                .view
                .detail
                .as_deref(),
            Some("尚未连接"),
            "没有条目时探测结果被丢弃"
        );
        assert_eq!(
            bus.max_seq().expect("取最大序号"),
            0,
            "未建立条目时不得广播"
        );
    }

    /// ADR 0041：管理写入在没有条目时建立它，并让后续探测写入可生效。
    #[test]
    fn begin_subscription_creates_entry() {
        let bus = Bus::new(16, 16);
        let laptop = name("laptop");

        let epoch = bus.begin_subscription(&laptop).expect("取视图纪元");
        let next = bus.begin_subscription(&laptop).expect("再次取视图纪元");
        assert!(next > epoch, "再次取纪元必须递增");
        assert_eq!(bus.max_seq().expect("取最大序号"), 0, "取纪元不广播");

        bus.set_probe_view(laptop.clone(), next, view(NodeStatus::Ok, ""))
            .expect("写入探测结果");
        assert_eq!(
            bus.node_entry(&laptop).expect("读取条目").view.status,
            NodeStatus::Ok,
            "条目已建立，探测写入生效"
        );
    }
}
