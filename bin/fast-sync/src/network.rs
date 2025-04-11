use std::sync::Arc;

use anyhow::bail;
use discv5::Enr;
use ethportal_api::{types::distance::Metric, ContentValue, OverlayContentKey};
use rand::{seq::SliceRandom, thread_rng};
use tracing::{error, warn};

use crate::{
    census::{
        peers::Peers,
        reputation::{update_reputation, REPUTATION_MIN_VALUE},
        Census,
    },
    protocol::Protocol,
    types::FindContentResult,
    utils::get_client_info,
};

#[derive(Clone)]
pub struct NetworkConfig {
    pub max_attempts: usize,
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

    pub async fn get_content_value<TContentValue: ContentValue<TContentKey = TContentKey>>(
        &self,
        content_key: TContentKey,
    ) -> anyhow::Result<TContentValue> {
        let mut peers = self.peers().interested_peers(&content_key.content_id());

        let on_rpc_result = |peer: &Enr, reputation: &mut f32, success: bool| {
            self.peers().record_rpc_result(&peer.node_id(), success);
            update_reputation(reputation, success);
        };

        let mut attempts = 0;
        while attempts < self.config.max_attempts {
            attempts += 1;
            let Ok((peer, reputation)) =
                peers.choose_weighted_mut(&mut thread_rng(), |(_peer, reputation)| *reputation)
            else {
                bail!(
                    "No peers available for content_key: {}",
                    content_key.to_hex(),
                );
            };
            match self.protocol.send_find_content(peer, &content_key).await {
                Ok(FindContentResult::Content(content_value_bytes)) => {
                    match TContentValue::decode(&content_key, &content_value_bytes) {
                        Ok(block_body) => {
                            on_rpc_result(peer, reputation, true);
                            return Ok(block_body);
                        }
                        Err(err) => {
                            error!(
                                bytes=?content_value_bytes,
                                "Can't decode content_value: {err:?}",
                            );
                            on_rpc_result(peer, reputation, false);
                        }
                    }
                }
                Ok(FindContentResult::Peers(other_peers)) => {
                    // Peer doesn't have content
                    self.census.peers_discovered(other_peers);
                    *reputation = REPUTATION_MIN_VALUE;
                }
                Err(err) => {
                    warn!(
                        peer_id=%peer.node_id(),
                        peer_client_info=get_client_info(peer),
                        "Error fetching content from peer: {err}",
                    );
                    on_rpc_result(peer, reputation, false);
                }
            }
        }

        bail!("Tried {attempts} times, content not fetched!",);
    }
}
