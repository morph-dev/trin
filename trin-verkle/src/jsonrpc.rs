use std::{fmt::Debug, sync::Arc};

use discv5::{enr::NodeId, Enr};
use ethportal_api::{
    jsonrpsee::core::Serialize,
    types::{
        distance::Distance,
        jsonrpc::{endpoints::VerkleEndpoint, request::VerkleJsonRpcRequest},
        portal::{AcceptInfo, FindNodesInfo, PongInfo},
        portal_wire::Content,
        query_trace::QueryTrace,
        verkle::{ContentInfo, TraceContentInfo},
    },
    utils::bytes::hex_encode,
    ContentValue, OverlayContentKey, VerkleContentKey, VerkleContentValue,
};
use portalnet::overlay::errors::OverlayRequestError;
use serde_json::{json, Value};
use tokio::sync::mpsc;
use tracing::error;
use trin_storage::ContentStore;

use crate::network::VerkleNetwork;

/// Handles Verkle network JSON-RPC requests
pub struct VerkleRequestHandler {
    network: Arc<VerkleNetwork>,
    verkle_rx: mpsc::UnboundedReceiver<VerkleJsonRpcRequest>,
}

impl VerkleRequestHandler {
    pub fn new(
        network: Arc<VerkleNetwork>,
        verkle_rx: mpsc::UnboundedReceiver<VerkleJsonRpcRequest>,
    ) -> Self {
        Self { network, verkle_rx }
    }

    pub async fn handle_client_queries(mut self) {
        while let Some(request) = self.verkle_rx.recv().await {
            let network = Arc::clone(&self.network);
            tokio::spawn(async move { Self::handle_request(network, request).await });
        }
    }

    async fn handle_request(network: Arc<VerkleNetwork>, request: VerkleJsonRpcRequest) {
        let response: Result<Value, String> = match request.endpoint {
            VerkleEndpoint::RoutingTableInfo => routing_table_info(network),
            VerkleEndpoint::Ping(enr) => ping(network, enr).await,
            VerkleEndpoint::AddEnr(enr) => add_enr(network, enr),
            VerkleEndpoint::DeleteEnr(node_id) => delete_enr(network, node_id),
            VerkleEndpoint::GetEnr(node_id) => get_enr(network, node_id),
            VerkleEndpoint::LookupEnr(node_id) => lookup_enr(network, node_id).await,
            VerkleEndpoint::FindNodes(enr, distances) => find_nodes(network, enr, distances).await,
            VerkleEndpoint::RecursiveFindNodes(node_id) => {
                recursive_find_nodes(network, node_id).await
            }
            VerkleEndpoint::DataRadius => radius(network),
            VerkleEndpoint::LocalContent(content_key) => local_content(network, content_key).await,
            VerkleEndpoint::FindContent(enr, content_key) => {
                find_content(network, enr, content_key).await
            }
            VerkleEndpoint::RecursiveFindContent(content_key) => {
                recursive_find_content(network, content_key, /* is_trace= */ false).await
            }
            VerkleEndpoint::TraceRecursiveFindContent(content_key) => {
                recursive_find_content(network, content_key, /* is_trace= */ true).await
            }
            VerkleEndpoint::Store(content_key, content_value) => {
                store(network, content_key, content_value).await
            }
            VerkleEndpoint::Offer(enr, content_key, content_value) => {
                offer(network, enr, content_key, content_value).await
            }
            VerkleEndpoint::Gossip(content_key, content_value) => {
                gossip(
                    network,
                    content_key,
                    content_value,
                    /* is_trace= */ false,
                )
                .await
            }
            VerkleEndpoint::TraceGossip(content_key, content_value) => {
                gossip(
                    network,
                    content_key,
                    content_value,
                    /* is_trace= */ true,
                )
                .await
            }
            VerkleEndpoint::PaginateLocalContentKeys(offset, limit) => {
                paginate(network, offset, limit)
            }
        };

        let _ = request.resp.send(response);
    }
}

fn routing_table_info(network: Arc<VerkleNetwork>) -> Result<Value, String> {
    serde_json::to_value(network.overlay.routing_table_info()).map_err(|err| err.to_string())
}

async fn ping(network: Arc<VerkleNetwork>, enr: Enr) -> Result<Value, String> {
    to_json_result(
        "Ping",
        network.overlay.send_ping(enr).await.map(|pong| PongInfo {
            enr_seq: pong.enr_seq,
            data_radius: *Distance::from(pong.custom_payload),
        }),
    )
}

fn add_enr(network: Arc<VerkleNetwork>, enr: Enr) -> Result<Value, String> {
    to_json_result("AddEnr", network.overlay.add_enr(enr).map(|_| true))
}

fn delete_enr(network: Arc<VerkleNetwork>, node_id: NodeId) -> Result<Value, String> {
    let is_deleted = network.overlay.delete_enr(node_id);
    Ok(json!(is_deleted))
}

fn get_enr(network: Arc<VerkleNetwork>, node_id: NodeId) -> Result<Value, String> {
    to_json_result("GetEnr", network.overlay.get_enr(node_id))
}

async fn lookup_enr(network: Arc<VerkleNetwork>, node_id: NodeId) -> Result<Value, String> {
    to_json_result("LookupEnr", network.overlay.lookup_enr(node_id).await)
}

fn radius(network: Arc<VerkleNetwork>) -> Result<Value, String> {
    let radius = network.overlay.data_radius();
    Ok(json!(*radius))
}

async fn find_nodes(
    network: Arc<VerkleNetwork>,
    enr: Enr,
    distances: Vec<u16>,
) -> Result<Value, String> {
    to_json_result(
        "FindNodes",
        network
            .overlay
            .send_find_nodes(enr, distances)
            .await
            .map(|nodes| {
                nodes
                    .enrs
                    .into_iter()
                    .map(Enr::from)
                    .collect::<FindNodesInfo>()
            }),
    )
}

async fn recursive_find_nodes(
    network: Arc<VerkleNetwork>,
    node_id: NodeId,
) -> Result<Value, String> {
    let nodes = network.overlay.lookup_node(node_id).await;
    Ok(json!(nodes))
}

fn local_storage_lookup(
    network: &Arc<VerkleNetwork>,
    content_key: &VerkleContentKey,
) -> Result<Option<Vec<u8>>, String> {
    network
        .overlay
        .store
        .read()
        .get(content_key)
        .map_err(|err| err.to_string())
}

async fn local_content(
    network: Arc<VerkleNetwork>,
    content_key: VerkleContentKey,
) -> Result<Value, String> {
    match local_storage_lookup(&network, &content_key) {
        Ok(Some(content)) => Ok(Value::String(hex_encode(content))),
        Ok(None) => {
            let err = json!({
                "message": "Content not found in local storage",
            });
            Err(err.to_string())
        }
        Err(err) => Err(format!(
            "LocalContent failed: error while looking for content key in local storage: {err:?}",
        )),
    }
}

async fn find_content(
    network: Arc<VerkleNetwork>,
    enr: Enr,
    content_key: VerkleContentKey,
) -> Result<Value, String> {
    let result = network
    .overlay
    .send_find_content(enr, content_key.into())
    .await
    .and_then(|(content, utp_transfer)| match content {
        Content::ConnectionId(id) => Err(OverlayRequestError::Failure(format!(
            "FindContent request returned a connection id ({id:?}) instead of conducting utp transfer."
        ))),
        Content::Content(content) => Ok(json!({
            "content": hex_encode(content),
            "utpTransfer": utp_transfer,
        })),
        Content::Enrs(enrs) => Ok(json!({
            "enrs": enrs,
        })),
    });
    to_json_result("FindContent", result)
}

async fn recursive_find_content(
    network: Arc<VerkleNetwork>,
    content_key: VerkleContentKey,
    is_trace: bool,
) -> Result<Value, String> {
    let local_content = match local_storage_lookup(&network, &content_key) {
        Ok(data) => data,
        Err(err) => {
            error!(
                error = %err,
                content.key = %content_key,
                "Error checking local store for content",
            );
            None
        }
    };
    let (content_bytes, utp_transfer, trace) = match local_content {
        Some(value) => {
            let trace = if is_trace {
                let local_enr = network.overlay.local_enr();
                let mut trace = QueryTrace::new(&local_enr, content_key.content_id());
                trace.node_responded_with_content(&local_enr);
                Some(trace)
            } else {
                None
            };
            (value, false, trace)
        }
        None => network
            .overlay
            .lookup_content(content_key.clone(), is_trace)
            .await
            .map_err(|err| err.to_string())?
            .map_err(|err| match err {
                OverlayRequestError::ContentNotFound {
                    message,
                    utp,
                    trace,
                } => {
                    let err = json!({
                        "message": format!("{message}: utp: {utp}"),
                        "trace": trace
                    });
                    err.to_string()
                }
                _ => {
                    error!(
                        error = %err,
                        content.key = %content_key,
                        "Error looking up content",
                    );
                    err.to_string()
                }
            })?,
    };

    let content =
        VerkleContentValue::decode(content_bytes.as_ref()).map_err(|err| err.to_string())?;

    if is_trace {
        Ok(json!(TraceContentInfo {
            content,
            utp_transfer,
            trace: trace.ok_or("Content query trace requested but none provided.".to_string())?,
        }))
    } else {
        Ok(json!(ContentInfo::Content {
            content: Box::new(content),
            utp_transfer
        }))
    }
}

async fn store(
    network: Arc<VerkleNetwork>,
    content_key: VerkleContentKey,
    content_value: VerkleContentValue,
) -> Result<Value, String> {
    to_json_result(
        "Store",
        network
            .overlay
            .store
            .write()
            .put(content_key, content_value.encode())
            .map(|_| true),
    )
}

async fn offer(
    network: Arc<VerkleNetwork>,
    enr: Enr,
    content_key: VerkleContentKey,
    content_value: Option<VerkleContentValue>,
) -> Result<Value, String> {
    if let Some(content_value) = content_value {
        to_json_result(
            "Populate Offer",
            network
                .overlay
                .send_populated_offer(enr, content_key.into(), content_value.encode())
                .await
                .map(|accept| AcceptInfo {
                    content_keys: accept.content_keys,
                }),
        )
    } else {
        to_json_result(
            "Offer",
            network
                .overlay
                .send_offer(vec![content_key.into()], enr)
                .await
                .map(|accept| AcceptInfo {
                    content_keys: accept.content_keys,
                }),
        )
    }
}

async fn gossip(
    network: Arc<VerkleNetwork>,
    content_key: VerkleContentKey,
    content_value: VerkleContentValue,
    is_trace: bool,
) -> Result<Value, String> {
    if is_trace {
        Ok(json!(
            network
                .overlay
                .propagate_gossip_trace(content_key, content_value.encode())
                .await
        ))
    } else {
        Ok(network
            .overlay
            .propagate_gossip(vec![(content_key, content_value.encode())])
            .into())
    }
}

fn paginate(network: Arc<VerkleNetwork>, offset: u64, limit: u64) -> Result<Value, String> {
    to_json_result(
        "PaginateLocalContentKeys",
        network.overlay.store.read().paginate(offset, limit),
    )
}

fn to_json_result(
    request: &str,
    result: Result<impl Serialize, impl Debug>,
) -> Result<Value, String> {
    result
        .map(|value| json!(value))
        .map_err(|err| format!("{request} failed: {err:?}"))
}
