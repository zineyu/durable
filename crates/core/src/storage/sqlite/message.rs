//! 消息读路径：树快照、单节点、transcript 回溯与按 id 批量取正文。
//! 写入在 `apply`，行解码在 `rows`。

use std::collections::HashMap;

use sqlx::Row;
use sqlx::sqlite::SqliteConnection;

use super::rows::{
    parse_message, parse_part_skeleton, parse_queue_item, parse_run, parse_session_head,
    parse_skeleton,
};
use super::{SESSION_HEAD_SQL, SqliteStorage, corrupted};
use crate::error::StoreError;
use crate::id::{MessageId, SessionId};
use crate::message::{ContentPart, Message, MessageBody, MessageNode, MessageSkeleton};
use crate::queue::QueueItem;
use crate::run::RunRecord;
use crate::storage::SessionSnapshot;

impl SqliteStorage {
    pub(crate) async fn session_tree(
        &self,
        id: &SessionId,
    ) -> Result<Option<SessionSnapshot>, StoreError> {
        let mut transaction = self.pool.begin().await?;
        let max_event_id: i64 = sqlx::query_scalar("SELECT COALESCE(MAX(id), 0) FROM event")
            .fetch_one(&mut *transaction)
            .await?;
        let row = sqlx::query(SESSION_HEAD_SQL)
            .bind(id.as_str())
            .fetch_optional(&mut *transaction)
            .await?;
        let Some(row) = row else {
            transaction.commit().await?;
            return Ok(None);
        };
        let head = parse_session_head(&row)?;
        let messages = session_messages(&mut transaction, id).await?;
        let queue = sqlx::query(
            "SELECT id, text, anchor, created_at, pending_steer FROM queue_item WHERE session_id = ?1 ORDER BY id ASC",
        )
        .bind(id.as_str())
        .fetch_all(&mut *transaction)
        .await?
        .iter()
        .map(parse_queue_item)
        .collect::<Result<Vec<QueueItem>, _>>()?;
        let runs = sqlx::query(
            "SELECT id, session_id, status, failure, created_at, updated_at FROM run
             WHERE session_id = ?1 ORDER BY id ASC",
        )
        .bind(id.as_str())
        .fetch_all(&mut *transaction)
        .await?
        .iter()
        .map(parse_run)
        .collect::<Result<Vec<RunRecord>, _>>()?;
        transaction.commit().await?;
        Ok(Some(SessionSnapshot {
            head,
            messages,
            queue,
            runs,
            max_event_id,
        }))
    }

    /// 指定 session 中的单个消息节点。
    pub(crate) async fn message(
        &self,
        session: &SessionId,
        id: MessageId,
    ) -> Result<Option<Message>, StoreError> {
        let row = sqlx::query(
            "SELECT id, parent_id, role, run_id, created_at FROM message
             WHERE session_id = ?1 AND id = ?2",
        )
        .bind(session.as_str())
        .bind(id.get())
        .fetch_optional(&self.pool)
        .await?;
        let Some(row) = row else {
            return Ok(None);
        };
        let mut connection = self.pool.acquire().await?;
        let parts = parts_of_message(&mut connection, id).await?;
        Ok(Some(Message {
            session: session.clone(),
            node: parse_message(&row, parts)?,
        }))
    }

    /// 从 active leaf 向上回溯到根或最近一条压缩消息，顺序为根在前。
    ///
    /// 按回溯深度倒序排列，不按 id 排序：深度 0 是 active leaf，深度越大越靠近根；
    /// 迁移 0006 会把旧消息重新挂到新铸的工具节点下，此后的 id 顺序不再等于链上顺序。
    pub(crate) async fn transcript(
        &self,
        session: &SessionId,
    ) -> Result<Vec<MessageNode>, StoreError> {
        let leaf: Option<i64> = sqlx::query_scalar("SELECT active_leaf FROM session WHERE id = ?1")
            .bind(session.as_str())
            .fetch_optional(&self.pool)
            .await?;
        let Some(leaf) = leaf else {
            return Ok(Vec::new());
        };
        let rows = sqlx::query(
            "WITH RECURSIVE chain(id, parent_id, role, run_id, created_at, depth) AS (
                 SELECT id, parent_id, role, run_id, created_at, 0 FROM message
                 WHERE session_id = ?2 AND id = ?1
                 UNION ALL
                 SELECT m.id, m.parent_id, m.role, m.run_id, m.created_at, c.depth + 1
                 FROM message m JOIN chain c ON m.id = c.parent_id
                 WHERE c.role <> 'compaction'
             )
             SELECT id, parent_id, role, run_id, created_at FROM chain ORDER BY depth DESC",
        )
        .bind(leaf)
        .bind(session.as_str())
        .fetch_all(&self.pool)
        .await?;
        let ids = ids_of(&rows)?;
        let mut connection = self.pool.acquire().await?;
        let mut parts = parts_of(&mut connection, session, &ids).await?;
        rows.iter()
            .zip(ids)
            .map(|(row, id)| parse_message(row, parts.remove(&id).unwrap_or_default()))
            .collect()
    }

    /// 按消息 id 批量取块正文；不属于该 session 的 id 被忽略。
    pub(crate) async fn message_bodies(
        &self,
        session: &SessionId,
        ids: &[MessageId],
    ) -> Result<Vec<MessageBody>, StoreError> {
        if ids.is_empty() {
            return Ok(Vec::new());
        }
        let rows = sqlx::query(
            "SELECT m.id AS id, p.body AS body FROM message m
             LEFT JOIN message_part p ON p.message_id = m.id
             WHERE m.session_id = ?1 AND m.id IN (SELECT value FROM json_each(?2))
             ORDER BY m.id ASC, p.seq ASC",
        )
        .bind(session.as_str())
        .bind(ids_json(ids)?)
        .fetch_all(&self.pool)
        .await?;
        let mut bodies: Vec<MessageBody> = Vec::new();
        for row in &rows {
            let id = MessageId::parse(row.get::<i64, _>("id"))
                .map_err(|error| corrupted("message.id", error))?;
            let part = row
                .get::<Option<String>, _>("body")
                .map(|body| {
                    serde_json::from_str::<ContentPart>(&body)
                        .map_err(|error| corrupted("message_part.body", error))
                })
                .transpose()?;
            match bodies.last_mut() {
                Some(last) if last.id == id => last.parts.extend(part),
                _ => bodies.push(MessageBody {
                    id,
                    parts: part.into_iter().collect(),
                }),
            }
        }
        Ok(bodies)
    }
}

/// 整个 session 的骨架：只读块的类型、状态与字节数，不读正文。
async fn session_messages(
    executor: &mut SqliteConnection,
    session: &SessionId,
) -> Result<Vec<MessageSkeleton>, StoreError> {
    let rows = sqlx::query(
        "SELECT m.id AS id, m.parent_id AS parent_id, m.role AS role, m.run_id AS run_id,
                m.created_at AS created_at, p.seq AS seq, p.kind AS kind, p.state AS state,
                length(p.body) AS size
         FROM message m LEFT JOIN message_part p ON p.message_id = m.id
         WHERE m.session_id = ?1
         ORDER BY m.id ASC, p.seq ASC",
    )
    .bind(session.as_str())
    .fetch_all(&mut *executor)
    .await?;
    let mut messages: Vec<MessageSkeleton> = Vec::new();
    for row in &rows {
        let node = parse_skeleton(row)?;
        match messages.last_mut() {
            Some(last) if last.id == node.id => {
                if row.get::<Option<i64>, _>("seq").is_some() {
                    last.parts.push(parse_part_skeleton(row)?);
                }
            }
            _ => {
                let mut started = node;
                if row.get::<Option<i64>, _>("seq").is_some() {
                    started.parts.push(parse_part_skeleton(row)?);
                }
                messages.push(started);
            }
        }
    }
    Ok(messages)
}

async fn parts_of(
    executor: &mut SqliteConnection,
    session: &SessionId,
    ids: &[MessageId],
) -> Result<HashMap<MessageId, Vec<ContentPart>>, StoreError> {
    let rows = sqlx::query(
        "SELECT p.message_id AS message_id, p.body AS body FROM message_part p
         JOIN message m ON m.id = p.message_id
         WHERE m.session_id = ?1 AND p.message_id IN (SELECT value FROM json_each(?2))
         ORDER BY p.message_id ASC, p.seq ASC",
    )
    .bind(session.as_str())
    .bind(ids_json(ids)?)
    .fetch_all(&mut *executor)
    .await?;
    let mut parts: HashMap<MessageId, Vec<ContentPart>> = HashMap::new();
    for row in &rows {
        let id = MessageId::parse(row.get::<i64, _>("message_id"))
            .map_err(|error| corrupted("message_part.message_id", error))?;
        let part: ContentPart = serde_json::from_str(&row.get::<String, _>("body"))
            .map_err(|error| corrupted("message_part.body", error))?;
        parts.entry(id).or_default().push(part);
    }
    Ok(parts)
}

async fn parts_of_message(
    executor: &mut SqliteConnection,
    id: MessageId,
) -> Result<Vec<ContentPart>, StoreError> {
    let rows = sqlx::query("SELECT body FROM message_part WHERE message_id = ?1 ORDER BY seq ASC")
        .bind(id.get())
        .fetch_all(&mut *executor)
        .await?;
    rows.iter()
        .map(|row| {
            serde_json::from_str::<ContentPart>(&row.get::<String, _>("body"))
                .map_err(|error| corrupted("message_part.body", error))
        })
        .collect()
}

fn ids_of(rows: &[sqlx::sqlite::SqliteRow]) -> Result<Vec<MessageId>, StoreError> {
    rows.iter()
        .map(|row| {
            MessageId::parse(row.get::<i64, _>("id"))
                .map_err(|error| corrupted("message.id", error))
        })
        .collect()
}

fn ids_json(ids: &[MessageId]) -> Result<String, StoreError> {
    Ok(serde_json::to_string(
        &ids.iter().map(|id| id.get()).collect::<Vec<_>>(),
    )?)
}
