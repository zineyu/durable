//! 消息树的问题：transcript、子树叶子与祖先查找。
//! 存储只返回行，链上顺序与压缩消息截断由本模块回答；写入在 `store`。

use std::collections::{BTreeMap, HashMap, HashSet};

use crate::id::MessageId;
use crate::message::{MessageRole, MessageSkeleton};

/// 一次查询内构建的消息树索引。
///
/// 构造时扫描一次节点集，建立「id 到节点」与「父节点到子节点」两张索引。索引只活在一次
/// 查询内，不跨查询缓存，因此没有失效逻辑。
#[derive(Debug)]
pub struct MessageTree {
    nodes: BTreeMap<MessageId, MessageSkeleton>,
    children: HashMap<MessageId, Vec<MessageId>>,
}

impl MessageTree {
    /// 从一次读出的全部消息骨架建立索引。
    ///
    /// 父指针指向不存在的节点时容忍：该节点当作根，并记录一条告警。
    /// 读路径不因为一条坏指针让整个 session 无法续跑。
    #[must_use]
    pub fn new(nodes: Vec<MessageSkeleton>) -> Self {
        let nodes: BTreeMap<MessageId, MessageSkeleton> =
            nodes.into_iter().map(|node| (node.id, node)).collect();
        let mut children: HashMap<MessageId, Vec<MessageId>> = HashMap::new();
        for node in nodes.values() {
            let Some(parent) = node.parent_id else {
                continue;
            };
            if !nodes.contains_key(&parent) {
                tracing::warn!(
                    message = node.id.get(),
                    parent = parent.get(),
                    "消息的父节点不存在，当作根处理"
                );
                continue;
            }
            children.entry(parent).or_default().push(node.id);
        }
        for ids in children.values_mut() {
            ids.sort_unstable();
        }
        Self { nodes, children }
    }

    /// 从给定节点向上找最近的满足谓词的祖先，不包含起点本身。
    #[must_use]
    pub fn ancestor(
        &self,
        start: MessageId,
        predicate: impl Fn(&MessageSkeleton) -> bool,
    ) -> Option<MessageId> {
        let mut current = self.nodes.get(&start)?.parent_id;
        let mut visited = HashSet::new();
        while let Some(id) = current {
            if !visited.insert(id) {
                return None;
            }
            let node = self.nodes.get(&id)?;
            if predicate(node) {
                return Some(id);
            }
            current = node.parent_id;
        }
        None
    }

    /// 直接子节点，按 id 升序。
    pub fn children_of(&self, parent: MessageId) -> impl Iterator<Item = &MessageSkeleton> + '_ {
        self.children
            .get(&parent)
            .into_iter()
            .flatten()
            .filter_map(|id| self.nodes.get(id))
    }

    /// 按消息 id 取节点。
    #[must_use]
    pub fn get(&self, id: MessageId) -> Option<&MessageSkeleton> {
        self.nodes.get(&id)
    }

    /// 从给定叶子沿父指针回溯到根或最近一条压缩消息，顺序为根在前。
    ///
    /// 压缩消息本身在结果里，比它更早的节点不出现。叶子不存在或父指针断开时在此截断。
    #[must_use]
    pub fn transcript(&self, leaf: MessageId) -> Vec<MessageSkeleton> {
        let mut chain: Vec<MessageSkeleton> = Vec::new();
        let mut visited = HashSet::new();
        let mut current = Some(leaf);
        while let Some(id) = current {
            if !visited.insert(id) {
                break;
            }
            let Some(node) = self.nodes.get(&id) else {
                break;
            };
            chain.push(node.clone());
            if node.role == MessageRole::Compaction {
                break;
            }
            current = node.parent_id;
        }
        chain.reverse();
        chain
    }

    /// 目标子树中最近创建的叶子（id 最大）。消息 id 全局单调递增，因此 id 最大即创建最晚。
    ///
    /// 目标本身是叶子时返回目标，目标不存在时返回 `None`。
    #[must_use]
    pub fn leaf_of(&self, target: MessageId) -> Option<MessageId> {
        let mut best: Option<MessageId> = None;
        let mut visited = HashSet::new();
        let mut stack = vec![target];
        while let Some(id) = stack.pop() {
            if !visited.insert(id) {
                continue;
            }
            if !self.nodes.contains_key(&id) {
                continue;
            }
            match self.children.get(&id) {
                Some(children) if !children.is_empty() => stack.extend(children.iter().copied()),
                _ => {
                    best = Some(match best {
                        None => id,
                        Some(current) => current.max(id),
                    });
                }
            }
        }
        best
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn node(id: i64, parent: Option<i64>, role: MessageRole) -> MessageSkeleton {
        MessageSkeleton {
            id: MessageId::parse(id).expect("合法消息 id"),
            parent_id: parent.map(|raw| MessageId::parse(raw).expect("合法父节点 id")),
            role,
            run_id: None,
            created_at: id,
            parts: Vec::new(),
        }
    }

    fn ids(chain: &[MessageSkeleton]) -> Vec<i64> {
        chain.iter().map(|node| node.id.get()).collect()
    }

    #[test]
    fn leaf_of_picks_the_largest_id_leaf_in_the_subtree() {
        let tree = MessageTree::new(vec![
            node(1, None, MessageRole::User),
            node(2, Some(1), MessageRole::Assistant),
            node(3, Some(1), MessageRole::User),
            node(4, Some(3), MessageRole::Assistant),
            node(5, Some(3), MessageRole::Assistant),
            node(6, Some(5), MessageRole::Tool),
        ]);

        assert_eq!(
            tree.leaf_of(MessageId::parse(1).expect("合法 id")),
            Some(MessageId::parse(6).expect("合法 id"))
        );
        assert_eq!(
            tree.leaf_of(MessageId::parse(2).expect("合法 id")),
            Some(MessageId::parse(2).expect("合法 id")),
            "目标本身是叶子时返回目标"
        );
        assert_eq!(
            tree.leaf_of(MessageId::parse(3).expect("合法 id")),
            Some(MessageId::parse(6).expect("合法 id")),
            "不把兄弟子树的叶子算进来"
        );
        assert_eq!(tree.leaf_of(MessageId::parse(9).expect("合法 id")), None);
    }

    #[test]
    fn transcript_orders_by_chain_not_by_id() {
        let tree = MessageTree::new(vec![
            node(1, None, MessageRole::User),
            node(2, Some(1), MessageRole::Assistant),
            node(3, Some(6), MessageRole::User),
            node(5, Some(2), MessageRole::Tool),
            node(6, Some(5), MessageRole::Tool),
        ]);

        let chain = tree.transcript(MessageId::parse(3).expect("合法 id"));

        assert_eq!(
            ids(&chain),
            vec![1, 2, 5, 6, 3],
            "旧库升级后 id 顺序不等于链上顺序"
        );
    }

    #[test]
    fn ancestor_excludes_the_start_and_uses_the_predicate() {
        let tree = MessageTree::new(vec![
            node(1, None, MessageRole::User),
            node(2, Some(1), MessageRole::Assistant),
            node(3, Some(2), MessageRole::User),
            node(4, Some(3), MessageRole::Assistant),
        ]);

        assert_eq!(
            tree.ancestor(MessageId::parse(4).expect("合法 id"), |node| node.role
                == MessageRole::User),
            Some(MessageId::parse(3).expect("合法 id"))
        );
        assert_eq!(
            tree.ancestor(MessageId::parse(3).expect("合法 id"), |node| node.role
                == MessageRole::User),
            Some(MessageId::parse(1).expect("合法 id")),
            "起点本身不算祖先"
        );
        assert_eq!(
            tree.ancestor(MessageId::parse(1).expect("合法 id"), |_| true),
            None
        );
    }

    #[test]
    fn children_of_lists_direct_children_by_ascending_id() {
        let tree = MessageTree::new(vec![
            node(1, None, MessageRole::User),
            node(2, Some(1), MessageRole::Assistant),
            node(5, Some(1), MessageRole::Assistant),
            node(6, Some(5), MessageRole::Tool),
            node(9, Some(99), MessageRole::User),
        ]);
        let parent = MessageId::parse(1).expect("合法 id");
        let children: Vec<i64> = tree.children_of(parent).map(|node| node.id.get()).collect();

        assert_eq!(children, vec![2, 5]);
        assert!(
            tree.children_of(MessageId::parse(99).expect("合法 id"))
                .next()
                .is_none()
        );
    }

    #[test]
    fn transcript_stops_at_the_compaction_message() {
        let tree = MessageTree::new(vec![
            node(1, None, MessageRole::User),
            node(2, Some(1), MessageRole::Assistant),
            node(3, Some(2), MessageRole::Compaction),
            node(4, Some(3), MessageRole::User),
        ]);

        let chain = tree.transcript(MessageId::parse(4).expect("合法 id"));

        assert_eq!(ids(&chain), vec![3, 4], "压缩消息在链首，更早的节点不出现");
    }

    #[test]
    fn transcript_stops_where_the_parent_is_missing() {
        let tree = MessageTree::new(vec![
            node(1, Some(9), MessageRole::User),
            node(2, Some(1), MessageRole::Assistant),
        ]);

        let chain = tree.transcript(MessageId::parse(2).expect("合法 id"));

        assert_eq!(ids(&chain), vec![1, 2], "断链处截断，已到达的节点保留");
    }

    #[test]
    fn empty_tree_answers_every_query_with_none() {
        let tree = MessageTree::new(Vec::new());
        let id = MessageId::parse(1).expect("合法 id");

        assert!(tree.get(id).is_none());
        assert!(tree.transcript(id).is_empty());
        assert_eq!(tree.leaf_of(id), None);
        assert_eq!(tree.ancestor(id, |_| true), None);
        assert!(tree.children_of(id).next().is_none());
    }

    #[test]
    fn transcript_of_unknown_leaf_is_empty() {
        let tree = MessageTree::new(vec![node(1, None, MessageRole::User)]);

        assert!(
            tree.transcript(MessageId::parse(7).expect("合法 id"))
                .is_empty()
        );
    }
}
