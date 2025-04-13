use std::{
    ffi::OsStr,
    fs::File,
    io::{BufReader, ErrorKind, Read},
    str::FromStr,
    sync::Arc,
    time::{Duration, Instant},
};

use alloy::primitives::B256;
use anyhow::bail;
use discv5::Enr;
use ethportal_api::{
    types::{distance::XorMetric, network::Subnetwork},
    HistoryContentKey, HistoryContentValue,
};
use humanize_duration::{prelude::DurationExt, Truncate};
use portalnet::discovery::UtpPeer;
use tokio::{sync::mpsc, time::interval};
use tracing::{debug, info, Level};
use utp_rs::socket::UtpSocket;

use crate::{
    census::Census,
    coordinator::CoordinatorConfig,
    discovery::Discovery,
    network::{Network, NetworkConfig},
    protocol::{Protocol, ProtocolConfig},
    utp_socket::Discovery5UtpSocket,
    Args,
};

const BOOTNODES: &[&str] = &[
    "enr:-Jy4QIs2pCyiKna9YWnAF0zgf7bT0GzlAGoF8MEKFJOExmtofBIqzm71zDvmzRiiLkxaEJcs_Amr7XIhLI74k1rtlXICY5Z0IDAuMS4xLWFscGhhLjEtMTEwZjUwgmlkgnY0gmlwhKEjVaWJc2VjcDI1NmsxoQLSC_nhF1iRwsCw0n3J4jRjqoaRxtKgsEe5a-Dz7y0JloN1ZHCCIyg",
    "enr:-Jy4QKSLYMpku9F0Ebk84zhIhwTkmn80UnYvE4Z4sOcLukASIcofrGdXVLAUPVHh8oPCfnEOZm1W1gcAxB9kV2FJywkCY5Z0IDAuMS4xLWFscGhhLjEtMTEwZjUwgmlkgnY0gmlwhJO2oc6Jc2VjcDI1NmsxoQLMSGVlxXL62N3sPtaV-n_TbZFCEM5AR7RDyIwOadbQK4N1ZHCCIyg",
    "enr:-Jy4QH4_H4cW--ejWDl_W7ngXw2m31MM2GT8_1ZgECnfWxMzZTiZKvHDgkmwUS_l2aqHHU54Q7hcFSPz6VGzkUjOqkcCY5Z0IDAuMS4xLWFscGhhLjEtMTEwZjUwgmlkgnY0gmlwhJ31OTWJc2VjcDI1NmsxoQPC0eRkjRajDiETr_DRa5N5VJRm-ttCWDoO1QAMMCg5pIN1ZHCCIyg",
];

pub struct Sync {
    args: Args,
    _discovery: Arc<Discovery>,
    _utp_socket: Arc<UtpSocket<UtpPeer>>,
    history: Arc<Network<HistoryContentKey, XorMetric>>,
}

impl Sync {
    pub async fn run(args: Args) -> anyhow::Result<()> {
        let block_hashes_path = args.block_hashes_path.clone();
        let block_hashes_future =
            tokio::spawn(async move { load_block_hashes(&block_hashes_path).await });

        let fast_sync = Self::create(&args).await?;
        fast_sync.heartbeat();

        let block_hashes = block_hashes_future.await??;

        fast_sync.fetch_block_bodies(block_hashes).await?;
        Ok(())
    }

    pub async fn create(args: &Args) -> anyhow::Result<Self> {
        // Setup discv5
        let discovery = Discovery::spawn(args.into()).await?;

        // Setup uTP
        let (utp_tx, utp_rx) = mpsc::channel(args.concurrency_utp);
        discovery.register_handler(Subnetwork::Utp, utp_tx);
        let utp_socket = Arc::new(UtpSocket::with_socket(Discovery5UtpSocket::new(
            &discovery, utp_rx,
        )));

        // Setup History Subnetwork
        let protocol = Protocol::<HistoryContentKey, XorMetric>::spawn(
            ProtocolConfig {
                incoming_talk_request_capacity: args.concurrency_in,
                outgoing_talk_request_capacity: args.concurrency_out,
            },
            Subnetwork::History,
            discovery.clone(),
            utp_socket.clone(),
        )?;

        let bootnodes = BOOTNODES
            .iter()
            .map(|bootnode| Enr::from_str(bootnode).unwrap())
            .collect::<Vec<_>>();
        let census = Census::<HistoryContentKey, XorMetric>::spawn(
            Arc::clone(&protocol),
            args.concurrency_census,
            &bootnodes,
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
            protocol,
            census,
        );

        Ok(Self {
            args: args.clone(),
            _discovery: discovery,
            _utp_socket: utp_socket,
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

pub async fn load_block_hashes(path: &OsStr) -> anyhow::Result<Vec<B256>> {
    info!("load_block_hashes: start");
    let mut reader = BufReader::new(File::open(path)?);
    let mut block_hashes = Vec::with_capacity(15537395);
    let mut buf = [0u8; 32];
    loop {
        if let Err(err) = reader.read_exact(&mut buf) {
            if err.kind() == ErrorKind::UnexpectedEof {
                break;
            }
            bail!("Error reading headers: {err}")
        }
        block_hashes.push(B256::from_slice(&buf));
    }
    info!("load_block_hashes: loaded: {}", block_hashes.len());
    Ok(block_hashes)
}
