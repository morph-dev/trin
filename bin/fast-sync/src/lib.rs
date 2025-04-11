use std::{
    ffi::{OsStr, OsString},
    fs::File,
    io::{BufReader, ErrorKind, Read},
    net::SocketAddr,
    str::FromStr,
    sync::Arc,
    time::{Duration, Instant},
};

use alloy::primitives::B256;
use anyhow::bail;
use census::Census;
use clap::{arg, Parser};
use discovery::Discovery;
use discv5::Enr;
use ethportal_api::{
    types::{distance::XorMetric, network::Subnetwork},
    HistoryContentKey, HistoryContentValue,
};
use futures::future::JoinAll;
use humanize_duration::{prelude::DurationExt, Truncate};
use network::{Network, NetworkConfig};
use portalnet::discovery::UtpPeer;
use protocol::{Protocol, ProtocolConfig};
use tokio::{sync::mpsc, time::interval};
use tracing::{error, info};
use utp_rs::socket::UtpSocket;
use utp_socket::Discovery5UtpSocket;

pub mod census;
pub mod discovery;
pub mod network;
pub mod protocol;
pub mod types;
pub mod utils;
pub mod utp_socket;

#[derive(Parser, Debug, Default, PartialEq, Eq, Clone)]
#[command(
    name = "fast-fetch",
    author = "https://github.com/ethereum/trin/graphs/contributors"
)]
pub struct Args {
    #[arg(help = "The block number of the first block to fetch.")]
    pub first_block: usize,

    #[arg(help = "The block number of the last bloc to fetch.")]
    pub last_block: usize,

    #[arg(long, help = "Path to binary file that stores block hashes")]
    pub block_hashes_path: OsString,

    #[arg(long)]
    pub private_key: Option<B256>,

    #[arg(long = "discovery-port", default_value_t = 9009)]
    pub discovery_port: u16,

    #[arg(long)]
    pub external_address: Option<SocketAddr>,

    #[arg(long = "discv5.enr_cache_capacity", default_value_t = 1000)]
    pub enr_cache_capacity: usize,

    #[arg(long = "discv5.timeout", default_value_t = 5000)]
    pub discv5_timeout_ms: u64,

    #[arg(long = "utp.concurrency", default_value_t = 1000)]
    pub utp_concurrency: usize,

    #[arg(long, default_value_t = 100)]
    pub concurrency: usize,

    #[arg(long, default_value_t = 1000)]
    pub batch_size: usize,

    #[arg(long, default_value_t = 20)]
    pub max_attempts: usize,
}

pub struct FastSync {
    args: Args,
    _discovery: Arc<Discovery>,
    _utp_socket: Arc<UtpSocket<UtpPeer>>,
    history: Arc<Network<HistoryContentKey, XorMetric>>,
}

const BOOTNODES_PUBLIC: &[&str] = &[
    "enr:-Jy4QIs2pCyiKna9YWnAF0zgf7bT0GzlAGoF8MEKFJOExmtofBIqzm71zDvmzRiiLkxaEJcs_Amr7XIhLI74k1rtlXICY5Z0IDAuMS4xLWFscGhhLjEtMTEwZjUwgmlkgnY0gmlwhKEjVaWJc2VjcDI1NmsxoQLSC_nhF1iRwsCw0n3J4jRjqoaRxtKgsEe5a-Dz7y0JloN1ZHCCIyg",
    "enr:-Jy4QKSLYMpku9F0Ebk84zhIhwTkmn80UnYvE4Z4sOcLukASIcofrGdXVLAUPVHh8oPCfnEOZm1W1gcAxB9kV2FJywkCY5Z0IDAuMS4xLWFscGhhLjEtMTEwZjUwgmlkgnY0gmlwhJO2oc6Jc2VjcDI1NmsxoQLMSGVlxXL62N3sPtaV-n_TbZFCEM5AR7RDyIwOadbQK4N1ZHCCIyg",
    "enr:-Jy4QH4_H4cW--ejWDl_W7ngXw2m31MM2GT8_1ZgECnfWxMzZTiZKvHDgkmwUS_l2aqHHU54Q7hcFSPz6VGzkUjOqkcCY5Z0IDAuMS4xLWFscGhhLjEtMTEwZjUwgmlkgnY0gmlwhJ31OTWJc2VjcDI1NmsxoQPC0eRkjRajDiETr_DRa5N5VJRm-ttCWDoO1QAMMCg5pIN1ZHCCIyg",
];

impl FastSync {
    pub async fn new(args: &Args) -> anyhow::Result<Self> {
        // Setup discv5
        let discovery = Discovery::spawn(args.into()).await?;

        // Setup uTP
        let (utp_tx, utp_rx) = mpsc::channel(args.utp_concurrency);
        discovery.register_handler(Subnetwork::Utp, utp_tx);
        let utp_socket = Arc::new(UtpSocket::with_socket(Discovery5UtpSocket::new(
            &discovery, utp_rx,
        )));

        // Setup History Subnetwork
        let bootnodes = BOOTNODES_PUBLIC
            .iter()
            .map(|bootnode| Enr::from_str(bootnode).unwrap())
            .collect::<Vec<_>>();

        let protocol = Protocol::<HistoryContentKey, XorMetric>::spawn(
            ProtocolConfig {
                incoming_talk_request_capacity: args.concurrency,
                outgoing_talk_request_capacity: args.concurrency,
            },
            Subnetwork::History,
            discovery.clone(),
            utp_socket.clone(),
        )?;

        let census = Census::<HistoryContentKey, XorMetric>::spawn(
            Arc::clone(&protocol),
            args.concurrency,
            &bootnodes,
        )
        .await?;

        let history = Network::new(
            NetworkConfig {
                max_attempts: args.max_attempts,
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
        tokio::spawn(async move {
            let mut heartbeat_interval = interval(Duration::from_secs(60));
            loop {
                heartbeat_interval.tick().await;
                info!("{}", peers.debug_table());
            }
        });
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
            let batch_end =
                usize::min(batch_start + self.args.batch_size - 1, self.args.last_block);

            let batch_start_time = Instant::now();
            let (success, failure) = self
                .fetch_block_bodies_batch(&block_hashes[batch_start..=batch_end])
                .await?;
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

    pub async fn fetch_block_bodies_batch(
        &self,
        block_hashes: &[B256],
    ) -> anyhow::Result<(usize, usize)> {
        let mut success = 0;
        let mut failure = 0;

        let block_bodies = block_hashes
            .iter()
            .map(|block_hash| async {
                let history = Arc::clone(&self.history);
                let block_hash = *block_hash;
                tokio::spawn(async move {
                    let content_key = HistoryContentKey::new_block_body(block_hash);
                    let block_body = history
                        .get_content_value::<HistoryContentValue>(content_key)
                        .await;
                    (block_hash, block_body)
                })
                .await
            })
            .collect::<JoinAll<_>>()
            .await;

        for task_result in block_bodies {
            let Ok((block_hash, block_body)) = task_result else {
                failure += 1;
                continue;
            };
            match block_body {
                Ok(_block_body) => success += 1,
                Err(err) => {
                    error!("Block body for {block_hash} not fetched: {err}");
                    failure += 1;
                }
            }
        }

        Ok((success, failure))
    }

    pub async fn run(args: Args) -> anyhow::Result<()> {
        let path = args.block_hashes_path.clone();
        let block_hashes_future = tokio::spawn(async move { load_block_hashes(&path).await });
        let fast_sync = Self::new(&args).await?;
        fast_sync.heartbeat();
        fast_sync
            .fetch_block_bodies(block_hashes_future.await??)
            .await?;
        Ok(())
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
