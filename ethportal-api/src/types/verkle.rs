use serde::{Deserialize, Serialize};

use crate::{types::enr::Enr, VerkleContentKey, VerkleContentValue};

use super::query_trace::QueryTrace;

/// Response for FindContent & RecursiveFindContent endpoints.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum ContentInfo {
    #[serde(rename_all = "camelCase")]
    ConnectionId { connection_id: u16 },
    #[serde(rename_all = "camelCase")]
    Content {
        content: VerkleContentValue,
        utp_transfer: bool,
    },
    #[serde(rename_all = "camelCase")]
    Enrs { enrs: Vec<Enr> },
}

/// Response for TraceRecursiveFindContent endpoint.
///
/// This struct represents the content info, and is only used when the content is found locally or
/// on the network.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TraceContentInfo {
    pub content: VerkleContentValue,
    pub utp_transfer: bool,
    pub trace: QueryTrace,
}

/// Response for PaginateLocalContentKeys endpoint
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PaginateLocalContentInfo {
    pub content_keys: Vec<VerkleContentKey>,
    pub total_entries: u64,
}
