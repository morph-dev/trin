#![warn(clippy::unwrap_used)]
#![warn(clippy::uninlined_format_args)]

use std::sync::Arc;

use ethportal_api::types::jsonrpc::request::VerkleJsonRpcRequest;
use jsonrpc::VerkleRequestHandler;
use portalnet::{
    config::PortalnetConfig,
    discovery::{Discovery, UtpEnr},
    events::{EventEnvelope, OverlayRequest},
};
use tokio::{
    sync::{broadcast, mpsc, RwLock},
    task::JoinHandle,
};
use trin_storage::PortalStorageConfig;
use trin_validation::oracle::HeaderOracle;
use utp_rs::socket::UtpSocket;

mod jsonrpc;

type VerkleHandler = Option<VerkleRequestHandler>;
type VerkleNetworkTask = Option<JoinHandle<()>>;
type VerkleEventTx = Option<mpsc::UnboundedSender<OverlayRequest>>;
type VerkleJsonRpcTx = Option<mpsc::UnboundedSender<VerkleJsonRpcRequest>>;
type VerkleEventStream = Option<broadcast::Receiver<EventEnvelope>>;

pub async fn initialize_verkle_network(
    _discovery: &Arc<Discovery>,
    _utp_socket: Arc<UtpSocket<UtpEnr>>,
    _portalnet_config: PortalnetConfig,
    _storage_config: PortalStorageConfig,
    _header_oracle: Arc<RwLock<HeaderOracle>>,
) -> anyhow::Result<(
    VerkleHandler,
    VerkleNetworkTask,
    VerkleEventTx,
    VerkleJsonRpcTx,
    VerkleEventStream,
)> {
    todo!()
}
