use discv5::enr::NodeId;
use ethportal_api::{
    types::{
        enr::Enr,
        jsonrpc::{endpoints::LegacyHistoryEndpoint, request::LegacyHistoryJsonRpcRequest},
        ping_extensions::{
            consts::LEGACY_HISTORY_SUPPORTED_EXTENSIONS, extension_types::PingExtensionType,
        },
        portal::{
            AcceptInfo, DataRadius, FindContentInfo, FindNodesInfo, GetContentInfo,
            PaginateLocalContentInfo, PongInfo, PutContentInfo, TraceContentInfo,
            MAX_CONTENT_KEYS_PER_OFFER,
        },
        portal_wire::OfferTrace,
    },
    ContentValue, LegacyHistoryContentKey, LegacyHistoryContentValue,
    LegacyHistoryNetworkApiServer, RawContentValue, RoutingTableInfo,
};
use serde_json::Value;
use tokio::sync::mpsc;

use crate::{
    errors::RpcServeError,
    fetch::proxy_to_subnet,
    jsonrpsee::core::{async_trait, RpcResult},
    ping_extension::parse_ping_payload,
};

pub struct LegacyHistoryNetworkApi {
    network: mpsc::UnboundedSender<LegacyHistoryJsonRpcRequest>,
}

impl LegacyHistoryNetworkApi {
    pub fn new(network: mpsc::UnboundedSender<LegacyHistoryJsonRpcRequest>) -> Self {
        Self { network }
    }
}

#[async_trait]
impl LegacyHistoryNetworkApiServer for LegacyHistoryNetworkApi {
    /// Returns meta information about overlay routing table.
    async fn routing_table_info(&self) -> RpcResult<RoutingTableInfo> {
        let endpoint = LegacyHistoryEndpoint::RoutingTableInfo;
        Ok(proxy_to_subnet(&self.network, endpoint).await?)
    }

    /// Write an Ethereum Node Record to the overlay routing table.
    async fn add_enr(&self, enr: Enr) -> RpcResult<bool> {
        let endpoint = LegacyHistoryEndpoint::AddEnr(enr);
        Ok(proxy_to_subnet(&self.network, endpoint).await?)
    }

    /// Fetch the latest ENR associated with the given node ID.
    async fn get_enr(&self, node_id: NodeId) -> RpcResult<Enr> {
        let endpoint = LegacyHistoryEndpoint::GetEnr(node_id);
        Ok(proxy_to_subnet(&self.network, endpoint).await?)
    }

    /// Delete Node ID from the overlay routing table.
    async fn delete_enr(&self, node_id: NodeId) -> RpcResult<bool> {
        let endpoint = LegacyHistoryEndpoint::DeleteEnr(node_id);
        Ok(proxy_to_subnet(&self.network, endpoint).await?)
    }

    /// Fetch the ENR representation associated with the given Node ID.
    async fn lookup_enr(&self, node_id: NodeId) -> RpcResult<Enr> {
        let endpoint = LegacyHistoryEndpoint::LookupEnr(node_id);
        Ok(proxy_to_subnet(&self.network, endpoint).await?)
    }

    /// Send a PING message to the designated node and wait for a PONG response
    async fn ping(
        &self,
        enr: Enr,
        payload_type: Option<PingExtensionType>,
        payload: Option<Value>,
    ) -> RpcResult<PongInfo> {
        let (payload_type, payload) =
            parse_ping_payload(LEGACY_HISTORY_SUPPORTED_EXTENSIONS, payload_type, payload)?;
        let endpoint = LegacyHistoryEndpoint::Ping(enr, payload_type, payload);
        Ok(proxy_to_subnet(&self.network, endpoint).await?)
    }

    /// Send a FINDNODES request for nodes that fall within the given set of distances, to the
    /// designated peer and wait for a response
    async fn find_nodes(&self, enr: Enr, distances: Vec<u16>) -> RpcResult<FindNodesInfo> {
        let endpoint = LegacyHistoryEndpoint::FindNodes(enr, distances);
        Ok(proxy_to_subnet(&self.network, endpoint).await?)
    }

    /// Lookup a target node within in the network
    async fn recursive_find_nodes(&self, node_id: NodeId) -> RpcResult<Vec<Enr>> {
        let endpoint = LegacyHistoryEndpoint::RecursiveFindNodes(node_id);
        Ok(proxy_to_subnet(&self.network, endpoint).await?)
    }

    /// Lookup a target node within in the network
    async fn radius(&self) -> RpcResult<DataRadius> {
        let endpoint = LegacyHistoryEndpoint::DataRadius;
        Ok(proxy_to_subnet(&self.network, endpoint).await?)
    }

    /// Send FINDCONTENT message to get the content with a content key.
    async fn find_content(
        &self,
        enr: Enr,
        content_key: LegacyHistoryContentKey,
    ) -> RpcResult<FindContentInfo> {
        let endpoint = LegacyHistoryEndpoint::FindContent(enr, content_key);
        Ok(proxy_to_subnet(&self.network, endpoint).await?)
    }

    /// First checks local storage if content is not found lookup a target content key in the
    /// network
    async fn get_content(&self, content_key: LegacyHistoryContentKey) -> RpcResult<GetContentInfo> {
        let endpoint = LegacyHistoryEndpoint::GetContent(content_key);
        Ok(proxy_to_subnet(&self.network, endpoint).await?)
    }

    /// First checks local storage if content is not found lookup a target content key in the
    /// network. Return tracing info.
    async fn trace_get_content(
        &self,
        content_key: LegacyHistoryContentKey,
    ) -> RpcResult<TraceContentInfo> {
        let endpoint = LegacyHistoryEndpoint::TraceGetContent(content_key);
        Ok(proxy_to_subnet(&self.network, endpoint).await?)
    }

    /// Pagination of local content keys
    async fn paginate_local_content_keys(
        &self,
        offset: u64,
        limit: u64,
    ) -> RpcResult<PaginateLocalContentInfo<LegacyHistoryContentKey>> {
        let endpoint = LegacyHistoryEndpoint::PaginateLocalContentKeys(offset, limit);
        Ok(proxy_to_subnet(&self.network, endpoint).await?)
    }

    /// Send the provided content to interested peers. Clients may choose to send to some or all
    /// peers. Return the number of peers that the content was gossiped to.
    async fn put_content(
        &self,
        content_key: LegacyHistoryContentKey,
        content_value: RawContentValue,
    ) -> RpcResult<PutContentInfo> {
        let content_value = LegacyHistoryContentValue::decode(&content_key, &content_value)
            .map_err(RpcServeError::from)?;
        let endpoint = LegacyHistoryEndpoint::PutContent(content_key, content_value);
        Ok(proxy_to_subnet(&self.network, endpoint).await?)
    }

    /// Send an OFFER request with given ContentItems, to the designated peer and wait for a
    /// response. Does not store content locally.
    /// Returns the content keys bitlist upon successful content transmission or empty bitlist
    /// receive.
    async fn offer(
        &self,
        enr: Enr,
        content_items: Vec<(LegacyHistoryContentKey, RawContentValue)>,
    ) -> RpcResult<AcceptInfo> {
        if !(1..=MAX_CONTENT_KEYS_PER_OFFER).contains(&content_items.len()) {
            return Err(RpcServeError::Message(format!(
                "Invalid amount of content items: {}",
                content_items.len()
            ))
            .into());
        }
        let content_items = content_items
            .into_iter()
            .map(|(key, value)| {
                LegacyHistoryContentValue::decode(&key, &value)
                    .map(|value| (key, value))
                    .map_err(RpcServeError::from)
            })
            .collect::<Result<Vec<_>, _>>()?;
        let endpoint = LegacyHistoryEndpoint::Offer(enr, content_items);
        Ok(proxy_to_subnet(&self.network, endpoint).await?)
    }

    /// Send an OFFER request with given ContentItems, to the designated peer.
    /// Does not store the content locally.
    /// Returns true if the content was accepted and successfully transferred,
    /// returns false if the content was not accepted or the transfer failed.
    async fn trace_offer(
        &self,
        enr: Enr,
        content_key: LegacyHistoryContentKey,
        content_value: RawContentValue,
    ) -> RpcResult<OfferTrace> {
        let content_value = LegacyHistoryContentValue::decode(&content_key, &content_value)
            .map_err(RpcServeError::from)?;
        let endpoint = LegacyHistoryEndpoint::TraceOffer(enr, content_key, content_value);
        Ok(proxy_to_subnet(&self.network, endpoint).await?)
    }

    /// Store content key with a content data to the local database.
    async fn store(
        &self,
        content_key: LegacyHistoryContentKey,
        content_value: RawContentValue,
    ) -> RpcResult<bool> {
        let content_value = LegacyHistoryContentValue::decode(&content_key, &content_value)
            .map_err(RpcServeError::from)?;
        let endpoint = LegacyHistoryEndpoint::Store(content_key, content_value);
        Ok(proxy_to_subnet(&self.network, endpoint).await?)
    }

    /// Get a content from the local database.
    async fn local_content(
        &self,
        content_key: LegacyHistoryContentKey,
    ) -> RpcResult<RawContentValue> {
        let endpoint = LegacyHistoryEndpoint::LocalContent(content_key);
        Ok(proxy_to_subnet(&self.network, endpoint).await?)
    }
}

impl std::fmt::Debug for LegacyHistoryNetworkApi {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("HistoryNetworkApi").finish_non_exhaustive()
    }
}
