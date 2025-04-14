use std::{
    sync::Arc,
    time::{Duration, Instant},
};

use alloy::primitives::B256;
use anyhow::bail;
use ethportal_api::{types::distance::XorMetric, HistoryContentKey, HistoryContentValue};
use humanize_duration::{prelude::DurationExt, Truncate};
use tokio::time::interval;
use tracing::{debug, info, Level};

use crate::{
    census::Census,
    coordinator::CoordinatorConfig,
    network::{Network, NetworkConfig},
    protocol::Protocol,
    utils::load_block_hashes,
    Args,
};

pub struct Sync {
    args: Args,
    history: Arc<Network<HistoryContentKey, XorMetric>>,
}

impl Sync {
    pub async fn run(
        args: Args,
        history: Arc<Protocol<HistoryContentKey, XorMetric>>,
    ) -> anyhow::Result<()> {
        let Some(block_hashes_path) = args.block_hashes_path.clone() else {
            bail!("block-hashes-path cli argument is required!");
        };

        let fast_sync = Self::create(&args, history).await?;
        fast_sync.heartbeat();

        fast_sync
            .fetch_block_bodies(load_block_hashes(&block_hashes_path)?)
            .await?;
        Ok(())
    }

    pub async fn create(
        args: &Args,
        history: Arc<Protocol<HistoryContentKey, XorMetric>>,
    ) -> anyhow::Result<Self> {
        let census = Census::<HistoryContentKey, XorMetric>::spawn(
            Arc::clone(&history),
            args.concurrency_census,
        )
        .await?;

        let history = Network::new(
            NetworkConfig {
                coordinator_config: CoordinatorConfig {
                    max_attempts_per_content: args.max_attempts,
                    concurrent_tasks_per_content: args.concurrency_per_content,
                    concurrent_tasks_per_peer: args.concurrency_per_peer,
                },
                concurrent_tasks: args.concurrency_out,
            },
            history,
            census,
        );

        Ok(Self {
            args: args.clone(),
            history: Arc::new(history),
        })
    }

    pub fn heartbeat(&self) {
        let peers = self.history.peers().clone();
        if tracing::enabled!(Level::DEBUG) {
            tokio::spawn(async move {
                let mut heartbeat_interval = interval(Duration::from_secs(60));
                loop {
                    heartbeat_interval.tick().await;
                    debug!("{}", peers.debug_table());
                }
            });
        }
    }

    pub async fn fetch_block_bodies(
        &self,
        block_hashes: Vec<B256>,
    ) -> anyhow::Result<(usize, usize)> {
        let start_time = Instant::now();

        let mut total_success = 0;
        let mut total_failure = 0;

        if self.args.first_block > self.args.last_block || self.args.last_block > block_hashes.len()
        {
            bail!(
                "Invalid block range: {}-{}",
                self.args.first_block,
                self.args.last_block,
            );
        }

        let mut batch_start = self.args.first_block;
        while batch_start <= self.args.last_block {
            let batch_start_time = Instant::now();

            let batch_end =
                usize::min(batch_start + self.args.batch_size - 1, self.args.last_block);
            let content_keys = block_hashes[batch_start..=batch_end]
                .iter()
                .map(|block_hash| HistoryContentKey::new_block_body(*block_hash))
                .collect();

            let content = self
                .history
                .batch_get_content::<HistoryContentValue>(content_keys)
                .await?;
            let (success, failure) = content.into_iter().fold(
                (0, 0),
                |(success, failure), (_content_key, content_value)| {
                    if content_value.is_some() {
                        (success + 1, failure)
                    } else {
                        (success, failure + 1)
                    }
                },
            );

            info!(
                "Finished block_bodies_batch {}-{} in {} = {}/{}",
                batch_start,
                batch_end,
                batch_start_time.elapsed().human(Truncate::Second),
                success,
                failure,
            );

            total_success += success;
            total_failure += failure;
            batch_start += self.args.batch_size;
        }

        info!(
            "Finished block_bodies {}-{} in {} = {}/{}",
            self.args.first_block,
            self.args.last_block,
            start_time.elapsed().human(Truncate::Second),
            total_success,
            total_failure,
        );
        Ok((total_success, total_failure))
    }
}
