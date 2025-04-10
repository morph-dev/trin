use std::sync::Arc;

use anyhow::bail;
use ethportal_api::{types::distance::Metric, ContentValue, OverlayContentKey};
use rand::{seq::SliceRandom, thread_rng};
use tracing::{error, warn};

use crate::{
    census::{peers::Peers, Census},
    protocol::Protocol,
    types::FindContentResult,
};

#[derive(Clone)]
pub struct NetworkConfig {
    pub max_attemps: usize,
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
        // TODO: report RPC status

        let peers = self.census.select_peers(&content_key.content_id())?;

        let mut weight = 1.0;
        let mut peers = peers
            .into_iter()
            .map(|(peer, _radius)| {
                weight /= 2.0;
                (peer, weight)
            })
            .collect::<Vec<_>>();

        let mut attempts = 0;
        while attempts < self.config.max_attemps {
            attempts += 1;
            let Ok((peer, peer_weight)) =
                peers.choose_weighted_mut(&mut thread_rng(), |(_peer, weight)| *weight)
            else {
                bail!(
                    "No peers available for content_key: {}",
                    content_key.to_hex(),
                );
            };
            match self.protocol.send_find_content(peer, &content_key).await {
                Ok(FindContentResult::Content(content_value_bytes)) => {
                    match TContentValue::decode(&content_key, &content_value_bytes) {
                        Ok(block_body) => return Ok(block_body),
                        Err(err) => {
                            error!(
                                bytes=?content_value_bytes,
                                "Can't decode content_value: {err:?}",
                            );
                            *peer_weight = 0.;
                        }
                    }
                }
                Ok(FindContentResult::Peers(_other_peers)) => {
                    // Peer doesn't have content
                    // Consider informing census of these peers.
                    // self.census.peers_discovered(other_peers);
                    *peer_weight = 0.;
                }
                Err(err) => {
                    warn!(
                        peer_id=%peer.node_id(),
                        peer_client_info=?peer.get_decodable::<String>("c").and_then(|client| client.ok()),
                        "Error fetching content from peer: {err}",
                    );
                    *peer_weight /= 2.;
                }
            }
        }
        bail!("Tried {attempts} times, content not fetched!");
    }
}
