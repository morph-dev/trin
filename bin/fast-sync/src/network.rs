use std::{sync::Arc, time::Duration};

use ethportal_api::{types::distance::Metric, ContentValue, OverlayContentKey};
use futures::future::JoinAll;
use tokio::sync::RwLock;
use tracing::error;

use crate::{
    census::{peers::Peers, Census},
    coordinator::{Coordinator, CoordinatorConfig, CreateTaskError, TaskResult},
    protocol::Protocol,
    types::FindContentResult,
    utils::get_client_info,
};

#[derive(Clone)]
pub struct NetworkConfig {
    pub coordinator_config: CoordinatorConfig,
    pub concurrent_tasks: usize,
}

pub struct Network<TContentKey, TMetric> {
    config: NetworkConfig,
    protocol: Arc<Protocol<TContentKey, TMetric>>,
    census: Arc<Census<TContentKey, TMetric>>,
}

impl<TContentKey, TMetric> Network<TContentKey, TMetric>
where
    TContentKey: 'static + Send + Sync + OverlayContentKey,
    TMetric: 'static + Send + Sync + Metric,
{
    pub fn new(
        config: NetworkConfig,
        protocol: Arc<Protocol<TContentKey, TMetric>>,
        census: Arc<Census<TContentKey, TMetric>>,
    ) -> Self {
        Self {
            config,
            protocol,
            census,
        }
    }

    pub fn protocol(&self) -> &Protocol<TContentKey, TMetric> {
        &self.protocol
    }

    pub fn census(&self) -> &Census<TContentKey, TMetric> {
        &self.census
    }

    pub fn peers(&self) -> &Peers {
        self.census.peers()
    }

    pub async fn batch_get_content<TContentValue>(
        &self,
        content_keys: Vec<TContentKey>,
    ) -> anyhow::Result<Vec<(TContentKey, Option<Box<TContentValue>>)>>
    where
        TContentValue: 'static + Send + Sync + ContentValue<TContentKey = TContentKey>,
    {
        let coordinator = Arc::new(RwLock::new(Coordinator::<TContentKey, TContentValue>::new(
            self.config.coordinator_config.clone(),
            content_keys.clone(),
            self.peers(),
        )));

        (0..self.config.concurrent_tasks)
            .map(|_| {
                let coordinator = Arc::clone(&coordinator);
                let census = Arc::clone(&self.census);
                let protocol = Arc::clone(&self.protocol);
                let peers = self.peers().clone();
                tokio::spawn(async move {
                    loop {
                        let create_task_result = coordinator.write().await.create_task::<TMetric>();
                        let task_info = match create_task_result {
                            Ok(task_info) => task_info,
                            Err(CreateTaskError::Busy) => {
                                tokio::time::sleep(Duration::from_secs(1)).await;
                                continue;
                            }
                            Err(CreateTaskError::NoPeers | CreateTaskError::NoContent) => {
                                return;
                            }
                        };

                        let task_result = match protocol
                            .send_find_content(&task_info.peer, &task_info.content_key)
                            .await
                        {
                            Ok(FindContentResult::Content(content_value)) => {
                                peers.record_rpc_result(
                                    &task_info.peer.node_id(),
                                    /* success= */ true,
                                );
                                TaskResult::Success(Box::new(content_value))
                            }
                            Ok(FindContentResult::Peers(other_peers)) => {
                                census.peers_discovered(other_peers);
                                TaskResult::ContentUnavailable
                            }
                            Err(err) => {
                                error!(
                                    peer_id=%task_info.peer.node_id(),
                                    peer_client_info=get_client_info(&task_info.peer),
                                    "Error fetching content from peer: {err}",
                                );
                                peers.record_rpc_result(
                                    &task_info.peer.node_id(),
                                    /* success= */ false,
                                );
                                TaskResult::Failure
                            }
                        };

                        coordinator
                            .write()
                            .await
                            .on_task_finish(task_info, task_result);
                    }
                })
            })
            .collect::<JoinAll<_>>()
            .await;

        let mut content = coordinator.write().await.get_content();
        Ok(content_keys
            .into_iter()
            .map(|content_key| {
                content
                    .remove(&content_key.content_id())
                    .unwrap_or_else(|| (content_key, None))
            })
            .collect())
    }
}
