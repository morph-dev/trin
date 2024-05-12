#![warn(clippy::unwrap_used)]
#![warn(clippy::uninlined_format_args)]

use std::{sync::Arc, time::Duration};

use discv5::Enr;
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
    time::interval,
};
use tracing::info;
use trin_storage::PortalStorageConfig;
use trin_validation::oracle::HeaderOracle;
use utp_rs::socket::UtpSocket;

use crate::{events::VerkleEvents, network::VerkleNetwork};

mod events;
mod jsonrpc;
mod network;
mod storage;
mod validator;

type VerkleHandler = Option<VerkleRequestHandler>;
type VerkleNetworkTask = Option<JoinHandle<()>>;
type VerkleEventTx = Option<mpsc::UnboundedSender<OverlayRequest>>;
type VerkleJsonRpcTx = Option<mpsc::UnboundedSender<VerkleJsonRpcRequest>>;
type VerkleEventStream = Option<broadcast::Receiver<EventEnvelope>>;

pub async fn initialize_verkle_network(
    discovery: &Arc<Discovery>,
    utp_socket: Arc<UtpSocket<UtpEnr>>,
    portalnet_config: PortalnetConfig,
    storage_config: PortalStorageConfig,
    header_oracle: Arc<RwLock<HeaderOracle>>,
) -> anyhow::Result<(
    VerkleHandler,
    VerkleNetworkTask,
    VerkleEventTx,
    VerkleJsonRpcTx,
    VerkleEventStream,
)> {
    let (verkle_jsonrpc_tx, verkle_jsonrpc_rx) = mpsc::unbounded_channel::<VerkleJsonRpcRequest>();
    let (verkle_event_tx, verkle_event_rx) = mpsc::unbounded_channel::<OverlayRequest>();

    let verkle_network = Arc::new(
        VerkleNetwork::new(
            Arc::clone(discovery),
            utp_socket,
            storage_config,
            portalnet_config.clone(),
            header_oracle,
        )
        .await?,
    );

    let verkle_handler = VerkleRequestHandler::new(Arc::clone(&verkle_network), verkle_jsonrpc_rx);
    let verkle_network_task = spawn_verkle_network(
        Arc::clone(&verkle_network),
        portalnet_config,
        verkle_event_rx,
    );
    let verkle_event_stream = verkle_network.overlay.event_stream().await?;

    spawn_verkle_heartbeat(verkle_network);

    Ok((
        Some(verkle_handler),
        Some(verkle_network_task),
        Some(verkle_event_tx),
        Some(verkle_jsonrpc_tx),
        Some(verkle_event_stream),
    ))
}

pub fn spawn_verkle_network(
    network: Arc<VerkleNetwork>,
    portalnet_config: PortalnetConfig,
    verkle_message_rx: mpsc::UnboundedReceiver<OverlayRequest>,
) -> JoinHandle<()> {
    let bootnode_enrs: Vec<Enr> = portalnet_config.bootnodes.into();
    info!(
        "About to spawn Verkle Network with {} boot nodes.",
        bootnode_enrs.len()
    );

    tokio::spawn(async move {
        let verkle_events = VerkleEvents {
            network: Arc::clone(&network),
            message_rx: verkle_message_rx,
        };

        // Spawn verkle event handler
        tokio::spawn(verkle_events.start());

        // hacky test: make sure we establish a session with the boot node
        network.overlay.ping_bootnodes().await;

        tokio::signal::ctrl_c()
            .await
            .expect("failed to pause until ctrl-c");
    })
}

pub fn spawn_verkle_heartbeat(network: Arc<VerkleNetwork>) {
    tokio::spawn(async move {
        let mut heart_interval = interval(Duration::from_millis(30000));

        loop {
            // Don't want to wait to display 1st log, but a bug seems to skip the first wait, so put
            // this wait at the top. Otherwise, we get two log lines immediately on startup.
            heart_interval.tick().await;

            let storage_log = network.overlay.store.read().get_summary_info();
            let message_log = network.overlay.get_message_summary();
            let utp_log = network.overlay.get_utp_summary();
            info!("reports~ data: {storage_log}; msgs: {message_log}");
            info!("reports~ utp: {utp_log}");
        }
    });
}
