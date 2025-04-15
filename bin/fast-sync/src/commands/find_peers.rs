use std::{collections::HashMap, fs::File, io::BufWriter, sync::Arc};

use anyhow::bail;
use ethportal_api::{
    types::distance::{Metric, XorMetric},
    HistoryContentKey, HistoryContentValue, OverlayContentKey,
};
use futures::future::JoinAll;
use itertools::Itertools;
use tokio::sync::RwLock;
use tracing::{debug, info, warn, Instrument};

use crate::{
    census::Census, protocol::Protocol, types::FindContentResult, utils::load_block_hashes, Args,
    FindPeersArgs,
};

pub async fn run(
    args: &Args,
    find_peers_args: &FindPeersArgs,
    history: Arc<Protocol<HistoryContentKey, XorMetric>>,
) -> anyhow::Result<()> {
    let census = Census::<HistoryContentKey, XorMetric>::spawn(
        Arc::clone(&history),
        args.concurrency_census,
    )
    .await?;
    let peers = Arc::new(RwLock::new(census.peers().get_all_alive()));

    let Some(block_hashes_path) = args.block_hashes_path.clone() else {
        bail!("block-hashes-path cli argument is required!");
    };
    let block_hashes = load_block_hashes(&block_hashes_path)?;
    let content = block_hashes[args.first_block..=args.last_block]
        .iter()
        .map(|block_hash| HistoryContentKey::new_block_body(*block_hash))
        .collect::<Vec<_>>();

    let peer_content = Arc::new(RwLock::new(HashMap::new()));

    let tasks = (0..args.concurrency_out)
        .map(|task_id| {
            let history = Arc::clone(&history);
            let peers = Arc::clone(&peers);
            let content = content.clone();
            let peer_content = Arc::clone(&peer_content);
            let content_per_peer = find_peers_args.content_per_peer;

            let task = async move {
                let mut total_success = 0;
                let mut total_unavailable = 0;
                let mut total_error = 0;
                loop {
                    let Some((peer, radius, _reputation)) = peers.write().await.pop() else {
                        return (total_success, total_unavailable, total_error);
                    };
                    let node_id = peer.node_id();
                    let raw_node_id = node_id.raw();

                    let content = content
                        .iter()
                        .filter_map(|content_key| {
                            let distance = XorMetric::distance(&raw_node_id, &content_key.content_id());
                            if distance < radius {
                                Some((content_key, distance))
                            } else {
                                None
                            }
                        })
                        .sorted_by_cached_key(|(_content_key, distance)| distance.big_endian_u32())
                        .collect::<Vec<_>>();

                    info!(
                        "Starting peer: {node_id:?} - eligible_content: {}",
                        content.len()
                    );

                    let mut success = vec![];
                    let mut unavailable = 0;
                    let mut error = 0;

                    for (content_key, _distance) in content.into_iter().take(content_per_peer) {
                        match history.send_find_content::<HistoryContentValue>(&peer, content_key).await {
                            Ok(FindContentResult::Content(_)) => {
                                success.push(content_key.clone());
                            }
                            Ok(FindContentResult::Peers(_)) => {
                                unavailable += 1;
                            }
                            Err(err) => {
                                warn!(
                                    %node_id,
                                    %content_key,
                                    "Error fetching content from peer: {err}",
                                );
                                error += 1;
                            }
                        }
                    }

                    total_success += success.len();
                    total_unavailable +=unavailable;
                    total_error += error;
                    info!(
                        "Finished peer: {node_id:?} - success={} unavailable={unavailable} error={error}",
                        success.len(),
                    );
                    if !success.is_empty() {
                        peer_content.write().await.insert(peer, success);
                    }
                }
            };
            tokio::spawn(task.instrument(tracing::info_span!("task", task_id)))
        })
        .collect::<JoinAll<_>>()
        .await;

    let mut total_success = 0;
    let mut total_unavailable = 0;
    let mut total_error = 0;
    for task in tasks.into_iter().enumerate() {
        match task {
            (_task_id, Ok((success, unavailable, error))) => {
                total_success += success;
                total_unavailable += unavailable;
                total_error += error;
            }
            (task_id, Err(err)) => {
                warn!(task_id, "Task failed: {err}");
            }
        }
    }

    let peer_content = peer_content.read().await.clone();

    info!(
        success = total_success,
        unavailable = total_unavailable,
        error = total_error,
        "Finished! Peers with content: {}",
        peer_content.len(),
    );
    debug!("\n{}", serde_json::to_string_pretty(&peer_content)?);

    let writer = BufWriter::new(File::create(&find_peers_args.output_file)?);
    serde_json::to_writer_pretty(writer, &peer_content)?;

    Ok(())
}
