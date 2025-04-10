use std::{marker::PhantomData, sync::Arc, time::Duration};

use anyhow::bail;
use discv5::{enr::NodeId, Enr};
use ethportal_api::{
    generate_random_node_ids,
    types::{
        distance::{Distance, Metric},
        ping_extensions::decode::PingExtension,
    },
    OverlayContentKey,
};
use futures::{future::JoinAll, StreamExt};
use itertools::Itertools;
use peers::Peers;
use tokio::{select, sync::Semaphore, time::Instant};
use tracing::{error, info, warn};

use crate::protocol::Protocol;

pub mod peer;
pub mod peers;

/// The result of the liveness check.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LivenessResult {
    /// We pinged the peer successfully
    Pass,
    /// We failed to ping peer
    Fail,
    /// Peer is already known and doesn't need liveness check
    Fresh,
}

/// The error that occured in [Census].
#[derive(Debug, thiserror::Error)]
pub enum CensusError {
    #[error("No peers found in Census")]
    NoPeers,
    #[error("Failed to initialize Census")]
    FailedInitialization,
    #[error("Census already initialized")]
    AlreadyInitialized,
}

#[derive(Clone)]
pub struct Census<TContentKey, TMetric> {
    protocol: Arc<Protocol<TContentKey, TMetric>>,
    peers: Peers,
    semaphore: Arc<Semaphore>,
    _phantom_content_key: PhantomData<TContentKey>,
    _phantom_metric: PhantomData<TMetric>,
}

impl<TContentKey, TMetric> Census<TContentKey, TMetric>
where
    TContentKey: 'static + Send + Sync + OverlayContentKey,
    TMetric: 'static + Send + Sync + Metric,
{
    const DISCOVERY_DEGREE: u32 = 4;
    const DISCOVERY_PEERS: usize = 5;
    const DISCOVERY_INTERVAL: Duration = Duration::from_secs(/* 10min= */ 600);
    const STOP_FRACTION_THRESHOLD: f64 = 0.01;

    pub async fn spawn(
        protocol: Arc<Protocol<TContentKey, TMetric>>,
        concurrency: usize,
        bootnodes: &[Enr],
    ) -> Result<Arc<Self>, CensusError> {
        let census = Arc::new(Self::new(protocol, concurrency));
        census.init(bootnodes).await?;
        census.start();
        Ok(census)
    }

    pub fn new(protocol: Arc<Protocol<TContentKey, TMetric>>, concurrency: usize) -> Self {
        Self {
            protocol,
            peers: Peers::new(),
            semaphore: Arc::new(Semaphore::new(concurrency)),
            _phantom_content_key: PhantomData,
            _phantom_metric: PhantomData,
        }
    }

    pub async fn init(&self, bootnodes: &[Enr]) -> Result<(), CensusError> {
        info!("init: started");

        bootnodes
            .iter()
            .map(|bootnode| async {
                match self.liveness_check(bootnode).await {
                    LivenessResult::Pass => info!("Bootnode {} is alive", bootnode.node_id()),
                    LivenessResult::Fail => warn!("Bootnode {} is NOT alive", bootnode.node_id()),
                    LivenessResult::Fresh => (),
                }
            })
            .collect::<JoinAll<_>>()
            .await;

        loop {
            let new_peers = self.peer_discovery().await;
            let ending_peers = self.peers.len();

            // Stop if number of new peers is less than a threshold fraction of all peers
            if (new_peers as f64) < Self::STOP_FRACTION_THRESHOLD * (ending_peers as f64) {
                break;
            }
        }

        if self.peers.is_empty() {
            error!("init: failed - couldn't find any peers",);
            return Err(CensusError::FailedInitialization);
        }

        info!("init: finished - found {} peers", self.peers.len(),);

        Ok(())
    }

    pub fn start(self: &Arc<Self>) {
        let census = Arc::clone(self);

        tokio::spawn(async move {
            let mut peers = census.peers.clone();

            let mut discovery_interval = tokio::time::interval(Self::DISCOVERY_INTERVAL);
            discovery_interval.reset();

            loop {
                select! {
                    _ = discovery_interval.tick() => {
                        info!("background_task: running peer discovery");
                        census.peer_discovery().await;
                    }
                    peer = peers.next() => {
                        match peer {
                            Some(peer) => {
                                info!("background_task: checking liveness: {}", peer.node_id());
                                census.liveness_check(&peer).await;
                            }
                            None => {
                                error!(
                                    "background_task: no pending peers. Stopping!",
                                );
                                break;
                            }
                        }
                    }
                };
            }
        });
    }

    async fn peer_discovery(&self) -> usize {
        // Generate random Node Ids
        let node_ids = generate_random_node_ids(Self::DISCOVERY_DEGREE);

        // Concurrent execution of FIND_NODES
        let results = node_ids
            .iter()
            .flat_map(|node_id| {
                self.closest_peers(node_id, Self::DISCOVERY_PEERS)
                    .unwrap_or_default()
                    .into_iter()
                    .map(move |peer| (peer, node_id))
            })
            .into_grouping_map()
            .collect::<Vec<&NodeId>>()
            .into_iter()
            .map(|(peer, node_ids)| {
                let semaphore = self.semaphore.clone();
                async move {
                    let Ok(_permit) = semaphore.acquire().await else {
                        bail!("failed to acquire permit")
                    };
                    let mut all_enrs = vec![];
                    for node_id in node_ids {
                        match self.protocol.send_find_nodes(&peer, node_id).await {
                            Ok(enrs) => {
                                self.record_rpc_result(&peer.node_id(), /* success= */ true);
                                all_enrs.extend(enrs)
                            }
                            Err(err) => {
                                self.record_rpc_result(&peer.node_id(), /* success= */ false);
                                error!(
                                    %err,
                                    peer_id=%peer.node_id(),
                                    peer_client_info=?peer.get_decodable::<String>("c").and_then(|client| client.ok()),
                                    "peer_discovery: FIND_NODES failed",
                                );
                            }
                        }
                    }
                    Ok(all_enrs)
                }
            })
            .collect::<JoinAll<_>>()
            .await;

        let enrs = results
            .into_iter()
            // Extract all ENRs
            .flat_map(|result| match result {
                Ok(enrs) => enrs,
                Err(err) => {
                    error!("peer_discovery: FIND_NODES failed - err: {err}",);
                    vec![]
                }
            })
            // Group by NodeId
            .into_grouping_map_by(|enr| enr.node_id())
            // Select ENR with maximum sequence number
            .max_by_key(|_node_id, enr| enr.seq())
            .into_values()
            .collect_vec();

        // Concurrent execution of liveness check
        let starting_peers = self.peers.len();
        enrs.iter()
            .map(|enr| async {
                if let Ok(_permit) = self.semaphore.acquire().await {
                    self.liveness_check(enr).await
                } else {
                    error!("init: liveness check failed - permit",);
                    LivenessResult::Fail
                }
            })
            .collect::<JoinAll<_>>()
            .await;
        let ending_peers = self.peers.len();
        let new_peers = ending_peers - starting_peers;

        info!("init: added {new_peers} / {ending_peers} peers",);

        new_peers
    }

    pub fn peers(&self) -> &Peers {
        &self.peers
    }

    /// Performs liveness check.
    ///
    /// Liveness check will pass if peer respond to a Ping request. It returns
    /// `LivenessResult::Fresh` if peer is already known and doesn't need liveness check.
    pub async fn liveness_check(&self, enr: &Enr) -> LivenessResult {
        // check if peer needs liveness check
        if self
            .peers
            .next_liveness_check(&enr.node_id())
            .is_some_and(|next_liveness_check| Instant::now() < next_liveness_check)
        {
            return LivenessResult::Fresh;
        }

        let Ok(pong) = self.protocol.send_ping(enr).await else {
            self.peers.record_failed_liveness_check(enr);
            return LivenessResult::Fail;
        };

        // If ENR seq is not the latest one, fetch fresh ENR
        let fresh_enr = if enr.seq() < pong.enr_seq {
            let Ok(enr) = self.protocol.send_get_enr(enr).await else {
                self.peers.record_failed_liveness_check(enr);
                return LivenessResult::Fail;
            };
            Some(enr)
        } else {
            if enr.seq() > pong.enr_seq {
                warn!(
                    "liveness_check: enr seq from pong ({}) is older than the one we know: {enr:?}",
                    pong.enr_seq
                );
            }
            None
        };
        let enr = fresh_enr.as_ref().unwrap_or(enr);

        let radius = match PingExtension::decode_ssz(pong.payload_type, pong.payload) {
            Ok(PingExtension::Capabilities(capabilities)) => capabilities.data_radius,
            Ok(PingExtension::HistoryRadius(history_radius)) => history_radius.data_radius,
            _ => {
                self.peers.record_failed_liveness_check(enr);
                return LivenessResult::Fail;
            }
        };

        self.peers.record_successful_liveness_check(enr, radius);
        LivenessResult::Pass
    }

    pub fn record_rpc_result(&self, peer: &NodeId, success: bool) {
        self.peers.record_rpc_result(peer, success);
    }

    pub fn peers_discovered(self: &Arc<Self>, peers: Vec<Enr>) {
        let census = self.clone();
        let unknown_peers = peers
            .into_iter()
            .filter(|peer| self.peers.get_peer(&peer.node_id()).is_none())
            .collect::<Vec<_>>();
        tokio::spawn(async move {
            for peer in unknown_peers {
                census.liveness_check(&peer).await;
            }
        });
    }

    pub fn closest_peers(&self, target: &NodeId, count: usize) -> Result<Vec<Enr>, CensusError> {
        if self.peers.is_empty() {
            return Err(CensusError::NoPeers);
        }
        Ok(self.peers.closest_peers(target, count))
    }

    pub fn select_peers(&self, content_id: &[u8; 32]) -> Result<Vec<(Enr, Distance)>, CensusError> {
        if self.peers.is_empty() {
            return Err(CensusError::NoPeers);
        }
        Ok(self.peers.select_peers(content_id))
    }

    pub fn debug_table(&self) -> String {
        self.peers.debug_table()
    }
}
