use discv5::enr::NodeId;

use crate::{
    types::{
        enr::Enr,
        ping_extensions::{decode::PingExtension, extension_types::PingExtensionType},
    },
    BeaconContentKey, BeaconContentValue, LegacyHistoryContentKey, LegacyHistoryContentValue,
    StateContentKey, StateContentValue,
};

/// Discv5 JSON-RPC endpoints. Start with "discv5_" prefix
#[derive(Debug, PartialEq, Eq, Clone)]
pub enum Discv5Endpoint {
    NodeInfo,
    RoutingTableInfo,
}

/// State network JSON-RPC endpoints. Start with "portal_state" prefix
#[derive(Debug, PartialEq, Eq, Clone)]
pub enum StateEndpoint {
    /// params: None
    RoutingTableInfo,
    /// params: [enr, payload_type, payload]
    Ping(Enr, Option<PingExtensionType>, Option<PingExtension>),
    /// params: [enr]
    AddEnr(Enr),
    /// params: [node_id]
    DeleteEnr(NodeId),
    /// params: [node_id]
    GetEnr(NodeId),
    /// params: [node_id]
    LookupEnr(NodeId),
    /// params: [enr, distances]
    FindNodes(Enr, Vec<u16>),
    /// params: [node_id]
    RecursiveFindNodes(NodeId),
    /// params: None
    DataRadius,
    /// params: content_key
    LocalContent(StateContentKey),
    /// params: [enr, content_key]
    FindContent(Enr, StateContentKey),
    /// params: content_key
    GetContent(StateContentKey),
    /// params: content_key
    TraceGetContent(StateContentKey),
    /// params: [content_key, content_value]
    Store(StateContentKey, StateContentValue),
    /// params: [enr, Vec<(content_key, content_value>)]
    Offer(Enr, Vec<(StateContentKey, StateContentValue)>),
    /// params: [enr, content_key, content_value]
    TraceOffer(Enr, StateContentKey, StateContentValue),
    /// params: [enr, content_key, content_value]
    PutContent(StateContentKey, StateContentValue),
    /// params: [offset, limit]
    PaginateLocalContentKeys(u64, u64),
}

/// History network JSON-RPC endpoints. Start with "portal_history" prefix
#[derive(Debug, PartialEq, Clone)]
pub enum LegacyHistoryEndpoint {
    /// params: [enr]
    AddEnr(Enr),
    /// params: None
    DataRadius,
    /// params: [node_id]
    DeleteEnr(NodeId),
    /// params: [enr, content_key]
    FindContent(Enr, LegacyHistoryContentKey),
    /// params: [enr, distances]
    FindNodes(Enr, Vec<u16>),
    /// params: [node_id]
    GetEnr(NodeId),
    /// params: content_key
    LocalContent(LegacyHistoryContentKey),
    /// params: [node_id]
    LookupEnr(NodeId),
    /// params: [content_key, content_value]
    PutContent(LegacyHistoryContentKey, LegacyHistoryContentValue),
    /// params: [enr, Vec<(content_key, content_value)>]
    Offer(
        Enr,
        Vec<(LegacyHistoryContentKey, LegacyHistoryContentValue)>,
    ),
    /// params: [enr, content_key, content_value]
    TraceOffer(Enr, LegacyHistoryContentKey, LegacyHistoryContentValue),
    /// params: [enr, payload_type, payload]
    Ping(Enr, Option<PingExtensionType>, Option<PingExtension>),
    /// params: content_key
    GetContent(LegacyHistoryContentKey),
    /// params: content_key
    TraceGetContent(LegacyHistoryContentKey),
    /// params: [content_key, content_value]
    Store(LegacyHistoryContentKey, LegacyHistoryContentValue),
    /// params: None
    RoutingTableInfo,
    // This endpoint is not History network specific
    /// params: [offset, limit]
    PaginateLocalContentKeys(u64, u64),
    /// params: [node_id]
    RecursiveFindNodes(NodeId),
}

/// Beacon network JSON-RPC endpoints. Start with "portal_beacon" prefix
#[derive(Debug, PartialEq, Clone)]
pub enum BeaconEndpoint {
    /// params: enr
    AddEnr(Enr),
    /// params: None
    DataRadius,
    /// params: node_id
    DeleteEnr(NodeId),
    /// params: None
    OptimisticStateRoot,
    /// params: [enr, content_key]
    FindContent(Enr, BeaconContentKey),
    /// params: [enr, distances]
    FindNodes(Enr, Vec<u16>),
    /// params: None
    FinalizedHeader,
    /// params: None
    FinalizedStateRoot,
    /// params: None
    FinalityUpdate,
    /// params: None
    OptimisticUpdate,
    /// params: node_id
    GetEnr(NodeId),
    /// params: None
    LightClientStore,
    /// params: content_key
    LocalContent(BeaconContentKey),
    /// params: node_id
    LookupEnr(NodeId),
    /// params: [content_key, content_value]
    PutContent(BeaconContentKey, BeaconContentValue),
    /// params: [enr, Vec<(content_key, content_value>)]
    Offer(Enr, Vec<(BeaconContentKey, BeaconContentValue)>),
    /// params: [enr, content_key, content_value]
    TraceOffer(Enr, BeaconContentKey, BeaconContentValue),
    /// params: [enr, payload_type, payload]
    Ping(Enr, Option<PingExtensionType>, Option<PingExtension>),
    /// params: content_key
    GetContent(BeaconContentKey),
    /// params: content_key
    TraceGetContent(BeaconContentKey),
    /// params: [content_key, content_value]
    Store(BeaconContentKey, BeaconContentValue),
    /// params: None
    RoutingTableInfo,
    /// params: [offset, limit]
    PaginateLocalContentKeys(u64, u64),
    /// params: [node_id]
    RecursiveFindNodes(NodeId),
}

/// The common functionality of subnetwork endpoints.
pub trait SubnetworkEndpoint {
    /// The subnetwork name.
    fn subnetwork() -> &'static str;
}

impl SubnetworkEndpoint for StateEndpoint {
    fn subnetwork() -> &'static str {
        "state"
    }
}

impl SubnetworkEndpoint for LegacyHistoryEndpoint {
    fn subnetwork() -> &'static str {
        "legacy_history"
    }
}

impl SubnetworkEndpoint for BeaconEndpoint {
    fn subnetwork() -> &'static str {
        "beacon"
    }
}
