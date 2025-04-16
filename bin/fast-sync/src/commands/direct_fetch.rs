use std::{fs::File, io::BufReader, sync::Arc, time::Instant};

use discv5::Enr;
use ethportal_api::{types::distance::XorMetric, HistoryContentKey, HistoryContentValue};
use futures::future::JoinAll;
use humanize_duration::{prelude::DurationExt, Truncate};
use rand::{seq::SliceRandom, thread_rng};
use tokio::sync::RwLock;
use tracing::{info, warn, Instrument};

use crate::{protocol::Protocol, types::FindContentResult, Args, DirectFetchArgs};

pub async fn run(
    args: &Args,
    direct_fetch_args: &DirectFetchArgs,
    history: Arc<Protocol<HistoryContentKey, XorMetric>>,
) -> anyhow::Result<()> {
    let timer = Instant::now();

    let reader = BufReader::new(File::open(&direct_fetch_args.input_file)?);

    // Read and shuffle peer_content vector
    let peer_content: Vec<(Enr, Vec<HistoryContentKey>)> = serde_json::from_reader(reader)?;
    let peer_content = Arc::new(RwLock::new(peer_content));
    peer_content.write().await.shuffle(&mut thread_rng());

    let tasks = (0..args.concurrency_out)
        .map(|task_id| {
            let history = Arc::clone(&history);
            let peer_content = Arc::clone(&peer_content);
            let content_per_peer = direct_fetch_args.content_per_peer;

            let task = async move {
                let mut total_success = 0;
                let mut total_unavailable = 0;
                let mut total_error = 0;
                loop {
                    let Some((peer, content_keys)) = peer_content.write().await.pop() else {
                        return (total_success, total_unavailable, total_error);
                    };

                    let mut success = 0;
                    let mut unavailable = 0;
                    let mut error = 0;

                    let node_id = peer.node_id();
                    let total_content_keys = content_keys.len();
                    info!("Starting peer: {node_id:?} - total_content: {total_content_keys}");
                    for content_key in content_keys
                        .into_iter()
                        .take(content_per_peer.unwrap_or(total_content_keys))
                    {
                        match history
                            .send_find_content::<HistoryContentValue>(&peer, &content_key)
                            .await
                        {
                            Ok(FindContentResult::Content(_)) => {
                                success += 1;
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
                    total_success += success;
                    total_unavailable += unavailable;
                    total_error += error;
                    info!(success, unavailable, error, "Finished peer: {node_id:?}",);
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

    info!(
        success = total_success,
        unavailable = total_unavailable,
        error = total_error,
        "Finished in {}",
        timer.elapsed().human(Truncate::Second),
    );
    Ok(())
}
