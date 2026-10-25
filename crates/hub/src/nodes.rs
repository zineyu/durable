//! hub 的节点表命令：注册、更新、删除与重连，以及节点快照。
//! 路由与节点作用域转发在父模块，上游订阅在 `bus`。

use axum::Json;
use axum::extract::rejection::JsonRejection;
use axum::extract::{FromRequest, Path as UrlPath, Request, State};
use axum::http::StatusCode;
use durable::{NodeMetadata, NodeName};
use serde::de::DeserializeOwned;

use crate::bus;
use crate::node_probe;
use crate::protocol::{NodeSnapshot, NodeStatus, RegisterNodeRequest, UpdateNodeRequest};
use crate::{ApiError, HubState};

/// 单个节点的快照。总线认为不可达的节点不再探测，直接回它的说明。
///
/// 探测失败只体现在这次快照里，不回写总线视图：探测结果只由 watcher 写（ADR 0028）。
/// 节点表命令另写停用与等待连接的管理状态（ADR 0041）。
/// 「这次能否取到元数据」与「上游事件流是否通」是两个观测，允许短暂不一致。
pub(crate) async fn snapshot_of(
    state: &HubState,
    node: NodeMetadata,
) -> Result<NodeSnapshot, ApiError> {
    let view = state.bus.view(&node.name)?;
    let metrics = state.bus.metrics(&node.name)?;
    Ok(match view.status {
        NodeStatus::Ok => {
            match node_probe::fetch_bootstrap(&state.client, &node, state.options.connect_timeout)
                .await
            {
                Ok(bootstrap) => NodeSnapshot::ok(node, bootstrap).with_metrics(metrics),
                Err(detail) => NodeSnapshot::failed(node, NodeStatus::Unreachable, detail)
                    .with_metrics(metrics),
            }
        }
        status => NodeSnapshot::failed(
            node,
            status,
            view.detail.unwrap_or_else(|| "节点不可用".to_owned()),
        )
        .with_metrics(metrics),
    })
}

/// 节点表命令的响应：以总线视图为准，不探测上游。
fn snapshot_of_view(state: &HubState, node: NodeMetadata) -> Result<NodeSnapshot, ApiError> {
    let view = state.bus.view(&node.name)?;
    let metrics = state.bus.metrics(&node.name)?;
    Ok(NodeSnapshot::of_view(node, view.status, view.detail).with_metrics(metrics))
}

pub(crate) async fn register_node(
    State(state): State<HubState>,
    JsonBody(request): JsonBody<RegisterNodeRequest>,
) -> Result<(StatusCode, Json<NodeSnapshot>), ApiError> {
    let name = NodeName::parse(&request.name)?;
    let url = validate_node_url(&request.url)?;
    let node = state.store.register_node(name, url).await?;
    state.bus.node_registered(node.clone())?;
    state.apply_enabled(node.clone())?;
    tracing::info!(node = %node.name, url = %node.url, "注册节点");
    Ok((StatusCode::CREATED, Json(snapshot_of_view(&state, node)?)))
}

/// 修改节点。请求体只带要改的字段：改名、换 URL、启用或停用。
///
/// 改名与既有语义一致：总线广播旧名的删除与新名的登记。其余变化广播 `node_updated`，
/// 并立即重建或停止上游订阅。
pub(crate) async fn update_node(
    State(state): State<HubState>,
    UrlPath(name): UrlPath<String>,
    JsonBody(request): JsonBody<UpdateNodeRequest>,
) -> Result<Json<NodeSnapshot>, ApiError> {
    let name = NodeName::parse(&name)?;
    let before = lookup(&state, &name).await?;
    let next = match request.name {
        Some(raw) => Some(NodeName::parse(&raw)?),
        None => None,
    };
    let url = match request.url {
        Some(raw) => Some(validate_node_url(&raw)?),
        None => None,
    };
    let after = state
        .store
        .update_node(
            &name,
            durable::NodeUpdate {
                name: next,
                url,
                enabled: request.enabled,
            },
        )
        .await?;

    if after.name != before.name {
        tracing::info!(from = %before.name, to = %after.name, "重命名节点");
        state.unwatch(&before.name)?;
        state.bus.node_removed(before.name)?;
        state.bus.node_registered(after.clone())?;
        state.apply_enabled(after.clone())?;
    } else if after != before {
        tracing::info!(node = %after.name, url = %after.url, enabled = after.enabled, "更新节点");
        state.bus.node_updated(after.clone())?;
        state.apply_enabled(after.clone())?;
    }
    Ok(Json(snapshot_of_view(&state, after)?))
}

/// 丢弃当前上游订阅并按退避起点立即重建。不影响节点视图，只在探测出结果时更新。
pub(crate) async fn reconnect_node(
    State(state): State<HubState>,
    UrlPath(name): UrlPath<String>,
) -> Result<StatusCode, ApiError> {
    let name = NodeName::parse(&name)?;
    let node = lookup(&state, &name).await?;
    if !node.enabled {
        return Err(ApiError::node_unavailable(bus::DISABLED_DETAIL.to_owned()));
    }
    tracing::info!(node = %node.name, "手动重连节点");
    state.watch(node)?;
    Ok(StatusCode::ACCEPTED)
}

pub(crate) async fn remove_node(
    State(state): State<HubState>,
    UrlPath(name): UrlPath<String>,
) -> Result<StatusCode, ApiError> {
    let name = NodeName::parse(&name)?;
    state.store.remove_node(&name).await?;
    tracing::info!(node = %name, "移除节点");
    state.unwatch(&name)?;
    state.bus.node_removed(name)?;
    Ok(StatusCode::NO_CONTENT)
}

pub(crate) async fn lookup(state: &HubState, name: &NodeName) -> Result<NodeMetadata, ApiError> {
    state
        .store
        .node(name)
        .await?
        .ok_or_else(|| ApiError::unknown_node(format!("节点不存在: {name}")))
}

/// 节点 URL 不能为空。前后空白在写入前去掉。
fn validate_node_url(raw: &str) -> Result<String, ApiError> {
    let url = raw.trim();
    if url.is_empty() {
        return Err(ApiError::invalid_request(
            StatusCode::UNPROCESSABLE_ENTITY,
            "节点 URL 不能为空".to_owned(),
        ));
    }
    Ok(url.to_owned())
}

/// 请求体；反序列化失败时返回协议错误信封而非纯文本。
pub(crate) struct JsonBody<T>(T);

impl<S, T> FromRequest<S> for JsonBody<T>
where
    T: DeserializeOwned,
    S: Send + Sync,
{
    type Rejection = ApiError;

    async fn from_request(request: Request, state: &S) -> Result<Self, Self::Rejection> {
        let Json(value) =
            Json::<T>::from_request(request, state)
                .await
                .map_err(|rejection: JsonRejection| {
                    ApiError::invalid_request(rejection.status(), rejection.body_text())
                })?;
        Ok(Self(value))
    }
}
