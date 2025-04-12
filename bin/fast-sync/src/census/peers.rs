use std::{
    collections::HashMap,
    pin::Pin,
    sync::{Arc, RwLock, RwLockReadGuard, RwLockWriteGuard},
    task::{Context, Poll},
    time::Duration,
};

use delay_map::HashSetDelay;
use discv5::{enr::NodeId, Enr};
use ethportal_api::types::distance::{Distance, Metric, XorMetric};
use futures::Stream;
use itertools::Itertools;
use tokio::time::Instant;
use tracing::error;

use super::peer::Peer;

/// How frequently liveness check should be done.
///
/// Five minutes is chosen arbitrarily.
const LIVENESS_CHECK_DELAY: Duration = Duration::from_secs(/* 5 min = */ 5 * 60);

/// Returns random duration `LIVENESS_CHECK_DELAY` and `2*LIVENESS_CHECK_DELAY`
fn get_random_liveness_check_delay() -> Duration {
    let default_delay = LIVENESS_CHECK_DELAY.as_secs_f64();
    let delay = (1. + rand::random::<f64>()) * default_delay;
    Duration::from_secs_f64(delay)
}

/// Stores peers and when they should be checked for liveness.
///
/// Convinient structure for holding both objects behind single [RwLock].
struct PeersWithLivenessChecks {
    /// Stores peers and their info
    peers: HashMap<NodeId, Peer>,
    /// Stores when peers should be checked for liveness using [HashSetDelay].
    liveness_checks: HashSetDelay<NodeId>,
}

/// Contains all discovered peers on the network.
///
/// It provides thread safe access to peers and is responsible for deciding when they should be
/// pinged for liveness.
#[derive(Clone)]
pub struct Peers {
    peers: Arc<RwLock<PeersWithLivenessChecks>>,
}

impl Peers {
    pub fn new() -> Self {
        Self {
            peers: Arc::new(RwLock::new(PeersWithLivenessChecks {
                peers: HashMap::new(),
                liveness_checks: HashSetDelay::new(LIVENESS_CHECK_DELAY),
            })),
        }
    }

    pub fn is_empty(&self) -> bool {
        self.read().peers.is_empty()
    }

    pub fn len(&self) -> usize {
        self.read().peers.len()
    }

    pub fn next_liveness_check(&self, node_id: &NodeId) -> Option<Instant> {
        self.read().liveness_checks.deadline(node_id)
    }

    pub fn record_successful_liveness_check(&self, enr: &Enr, radius: Distance) {
        let node_id = enr.node_id();
        let mut guard = self.write();
        guard
            .peers
            .entry(node_id)
            .or_insert_with(|| Peer::new(enr.clone()))
            .record_successful_liveness_check(enr.clone(), radius);
        guard
            .liveness_checks
            .insert_at(node_id, get_random_liveness_check_delay());
    }

    pub fn record_failed_liveness_check(&self, enr: &Enr) {
        let node_id = enr.node_id();
        let mut guard = self.write();
        let peer = guard
            .peers
            .entry(node_id)
            .or_insert_with(|| Peer::new(enr.clone()));
        peer.record_failed_liveness_check();

        if peer.is_obsolete() {
            guard.liveness_checks.remove(&node_id);
        } else {
            guard
                .liveness_checks
                .insert_at(node_id, get_random_liveness_check_delay());
        }
    }

    pub fn record_rpc_result(&self, peer: &NodeId, success: bool) {
        if let Some(peer) = self.write().peers.get_mut(peer) {
            peer.record_rpc_result(success);
        }
    }

    pub fn get_peer(&self, peer: &NodeId) -> Option<(Enr, Distance)> {
        self.read()
            .peers
            .get(peer)
            .map(|peer| (peer.enr(), peer.radius()))
    }

    pub fn get_all_alive(&self) -> Vec<(Enr, Distance, f32)> {
        self.read()
            .peers
            .values()
            .filter(|peer| peer.is_alive())
            .map(|peer| (peer.enr(), peer.radius(), peer.reputation()))
            .collect()
    }

    pub fn closest_peers(&self, target: &NodeId, count: usize) -> Vec<Enr> {
        let target = target.raw();
        self.read()
            .peers
            .values()
            .filter(|peer| peer.is_alive())
            .sorted_by_cached_key(|peer| {
                XorMetric::distance(&peer.node_id().raw(), &target).big_endian_u32()
            })
            .take(count)
            .map(|peer| peer.enr())
            .collect()
    }

    /// Returns all alive peers that could have the content.
    ///
    /// Result contains peers (`Enr` and reputation), and is sorted by distance from content id.
    pub fn interested_peers(&self, content_id: &[u8; 32]) -> Vec<(Enr, f32)> {
        let peers = self
            .read()
            .peers
            .values()
            .filter(|peer| peer.is_alive() && peer.is_content_within_radius(content_id))
            .sorted_by_cached_key(|peer| {
                XorMetric::distance(&peer.node_id().raw(), content_id).big_endian_u32()
            })
            .map(|peer| (peer.enr(), peer.reputation()))
            .collect();
        peers
    }

    fn read(&self) -> RwLockReadGuard<'_, PeersWithLivenessChecks> {
        self.peers.read().expect("to get peers lock")
    }

    fn write(&self) -> RwLockWriteGuard<'_, PeersWithLivenessChecks> {
        self.peers.write().expect("to get peers lock")
    }

    pub fn debug_table(&self) -> String {
        let rows = self
            .read()
            .peers
            .values()
            .sorted_by(|a, b| b.reputation().partial_cmp(&a.reputation()).unwrap())
            .map(|peer| {
                format!(
                    "{:6.2} | {:5} | {:10} | {:12} | 0x{:08x} | {:7}/{:<7} ",
                    peer.reputation(),
                    peer.is_alive(),
                    peer.client_info(),
                    peer.node_id(),
                    peer.radius().big_endian_u32(),
                    peer.rpc_success(),
                    peer.rpc_failure(),
                )
            })
            .join("\n");
        format!("\nPeers:\n score | alive |     client |      node_id |     radius | success/failure\n{rows}")
    }
}

impl Stream for Peers {
    type Item = Enr;

    fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        let mut guard = self.write();

        // Poll expired until non-error is returned.
        // Error can happen only if there is some race condition, which shouldn't happen because
        // of the RwLock.
        loop {
            match guard.liveness_checks.poll_expired(cx) {
                Poll::Ready(Some(Ok(node_id))) => match guard.peers.get(&node_id) {
                    Some(peer) => break Poll::Ready(Some(peer.enr())),
                    None => {
                        error!("poll_next: unknown peer: {node_id}");
                    }
                },
                Poll::Ready(Some(Err(err))) => {
                    error!("poll_next: error getting peer - err: {err}");
                }
                Poll::Ready(None) => break Poll::Ready(None),
                Poll::Pending => break Poll::Pending,
            }
        }
    }
}

impl Default for Peers {
    fn default() -> Self {
        Self::new()
    }
}
