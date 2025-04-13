use std::{
    collections::{HashMap, HashSet},
    sync::Arc,
};

use alloy::primitives::B256;
use discv5::{enr::NodeId, Enr};
use ethportal_api::{
    types::distance::{Distance, Metric},
    ContentValue, OverlayContentKey,
};
use rand::{seq::SliceRandom, thread_rng};
use tracing::error;

use crate::census::{peers::Peers, reputation::update_reputation};

const IN_PROGRESS_REPUTATION_PENALTY: f32 = 0.5;

pub struct ContentInfo<K, V> {
    pub content_id: B256,
    pub content_key: K,
    pub content_value: Option<Box<V>>,
    pub active_tasks: usize,
    pub total_tasks: usize,
}

impl<K, V> ContentInfo<K, V>
where
    K: OverlayContentKey,
    V: ContentValue<TContentKey = K>,
{
    pub fn new(content_key: K) -> Self {
        Self {
            content_id: B256::new(content_key.content_id()),
            content_key,
            content_value: None,
            active_tasks: 0,
            total_tasks: 0,
        }
    }
}

pub struct PeerInfo {
    pub enr: Arc<Enr>,
    pub radius: Distance,
    pub reputation: f32,
    pub useful: bool,
    pub content_in_progress: HashSet<B256>,
    pub unavailable_content: HashSet<B256>,
}

impl PeerInfo {
    pub fn new(enr: Arc<Enr>, radius: Distance, reputation: f32) -> Self {
        Self {
            enr,
            radius,
            reputation,
            useful: true,
            content_in_progress: HashSet::new(),
            unavailable_content: HashSet::new(),
        }
    }
}

pub enum TaskResult<V> {
    Success(Box<V>),
    ContentUnavailable,
    Failure,
}

pub struct TaskInfo<K> {
    pub peer: Arc<Enr>,
    pub content_id: B256,
    pub content_key: K,
}

#[derive(Clone, Debug, thiserror::Error)]
pub enum CreateTaskError {
    #[error("There is no content that should be fetched")]
    NoContent,
    #[error("There is content that should be fetched, but no useful peers")]
    NoPeers,
    #[error("All useful peers are busy")]
    Busy,
}

#[derive(Clone, Debug)]
pub struct CoordinatorConfig {
    pub max_attempts_per_content: usize,
    pub concurrent_tasks_per_content: usize,
    pub concurrent_tasks_per_peer: usize,
}

/// Coordinates content fetching from the network
pub struct Coordinator<K, V> {
    config: CoordinatorConfig,
    pending_content: HashMap<B256, ContentInfo<K, V>>,
    finished_content: HashMap<B256, (K, Option<Box<V>>)>,
    peers: HashMap<NodeId, PeerInfo>,
}

impl<K, V> Coordinator<K, V>
where
    K: OverlayContentKey,
    V: ContentValue<TContentKey = K>,
{
    pub fn new(config: CoordinatorConfig, content_keys: Vec<K>, peers: &Peers) -> Self {
        let pending_content = content_keys
            .into_iter()
            .map(ContentInfo::new)
            .map(|content_info| (content_info.content_id, content_info))
            .collect::<HashMap<_, _>>();
        let finished_content = HashMap::with_capacity(pending_content.len());
        let peers = peers
            .get_all_alive()
            .into_iter()
            .map(|(enr, radius, reputation)| {
                (
                    enr.node_id(),
                    PeerInfo::new(Arc::new(enr), radius, reputation),
                )
            })
            .collect();
        Self {
            config,
            pending_content,
            finished_content,
            peers,
        }
    }

    pub fn create_task<TMetric: Metric>(&mut self) -> Result<TaskInfo<K>, CreateTaskError> {
        let mut rng = thread_rng();

        // Make sure that there is content pending to be fetched.
        if self.pending_content.is_empty() {
            return Err(CreateTaskError::NoContent);
        }

        // Select peers that are:
        // 1. useful (there is some pending content in radius)
        // 2. can execute new task
        let mut eligible_peers = self
            .peers
            .values_mut()
            .filter(|peer| {
                peer.useful
                    && peer.content_in_progress.len() < self.config.concurrent_tasks_per_peer
            })
            .collect::<Vec<_>>();

        while !eligible_peers.is_empty() {
            // Select peer based on their reputation, adjusted for tasks in progress
            let peer_info = eligible_peers
                .choose_weighted_mut(&mut rng, |peer_info| {
                    peer_info.reputation
                        * IN_PROGRESS_REPUTATION_PENALTY
                            .powi(peer_info.content_in_progress.len() as i32)
                })
                .expect("should be able to select peer");

            // Find all content that peer is interested in and we can attempt.
            let mut interested_content = self
                .pending_content
                .values_mut()
                .filter(|content_info| {
                    let distance =
                        TMetric::distance(&peer_info.enr.node_id().raw(), &content_info.content_id);
                    let in_radius = distance <= peer_info.radius;
                    let can_attempt =
                        content_info.total_tasks < self.config.max_attempts_per_content;
                    in_radius && can_attempt
                })
                .collect::<Vec<_>>();

            // Mark peer as not-useful if no such content
            if interested_content.is_empty() {
                peer_info.useful = false;
            }

            // Remove content that are currently on their capacity
            interested_content.retain(|content_info| {
                content_info.active_tasks < self.config.concurrent_tasks_per_content
            });

            // Peer can't attempt any of the content at the moment. Skip this peer.
            if interested_content.is_empty() {
                let node_id = peer_info.enr.node_id();
                eligible_peers.retain_mut(|peer_info| peer_info.enr.node_id() != node_id);
                continue;
            }

            // Select random content, adjusted for tasks in progress
            let content_info = interested_content
                .choose_weighted_mut(&mut rng, |content_info| {
                    IN_PROGRESS_REPUTATION_PENALTY.powi(content_info.active_tasks as i32)
                })
                .expect("should be able to select content");

            // Create task
            peer_info
                .content_in_progress
                .insert(content_info.content_id);
            content_info.active_tasks += 1;
            content_info.total_tasks += 1;
            return Ok(TaskInfo {
                peer: Arc::clone(&peer_info.enr),
                content_id: content_info.content_id,
                content_key: content_info.content_key.clone(),
            });
        }

        if self.peers.values().any(|peer_info| peer_info.useful) {
            Err(CreateTaskError::Busy)
        } else {
            Err(CreateTaskError::NoPeers)
        }
    }

    pub fn on_task_finish(&mut self, task: TaskInfo<K>, result: TaskResult<V>) {
        let Some(peer_info) = self.peers.get_mut(&task.peer.node_id()) else {
            error!("Couldn't find peer: {}", task.peer.node_id());
            return;
        };

        peer_info.content_in_progress.remove(&task.content_id);

        match result {
            TaskResult::Success(content_value) => {
                update_reputation(&mut peer_info.reputation, /* success= */ true);
                self.pending_content.remove(&task.content_id);
                self.finished_content
                    .insert(task.content_id, (task.content_key, Some(content_value)));
                return;
            }
            TaskResult::ContentUnavailable => {
                peer_info.unavailable_content.insert(task.content_id);
            }
            TaskResult::Failure => {
                update_reputation(&mut peer_info.reputation, /* success= */ false);
            }
        }
        if let Some(content_info) = self.pending_content.get_mut(&task.content_id) {
            content_info.active_tasks -= 1;
            if content_info.total_tasks >= self.config.max_attempts_per_content {
                self.pending_content.remove(&task.content_id);
                self.finished_content
                    .insert(task.content_id, (task.content_key, None));
            }
        }
    }

    pub fn get_content(&mut self) -> HashMap<B256, (K, Option<Box<V>>)> {
        if !self.pending_content.is_empty() {
            panic!(
                "Pending content still present! {} {}",
                self.pending_content.len(),
                self.finished_content.len()
            );
        }
        self.finished_content.drain().collect()
    }
}
