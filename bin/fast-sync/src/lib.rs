use std::{ffi::OsString, net::SocketAddr, sync::Arc};

use alloy::primitives::B256;
use clap::{Args as ClapArgs, Parser, Subcommand};
use discovery::{Discovery, DiscoveryConfig};
use ethportal_api::{
    types::{distance::XorMetric, network::Subnetwork},
    HistoryContentKey,
};
use protocol::{Protocol, ProtocolConfig};
use sync::Sync;
use tokio::sync::mpsc;
use utp_rs::socket::UtpSocket;
use utp_socket::Discovery5UtpSocket;

pub mod census;
pub mod commands;
pub mod coordinator;
pub mod discovery;
pub mod network;
pub mod protocol;
pub mod sync;
pub mod types;
pub mod utils;
pub mod utp_socket;

#[derive(Parser, Clone, Debug, PartialEq, Eq)]
#[command(
    name = "fast-fetch",
    author = "https://github.com/ethereum/trin/graphs/contributors"
)]
pub struct Args {
    #[arg(
        help = "The block number of the first block to fetch.",
        default_value_t = 0
    )]
    pub first_block: usize,

    #[arg(
        help = "The block number of the last bloc to fetch.",
        default_value_t = 0
    )]
    pub last_block: usize,

    #[arg(long, help = "Path to binary file that stores block hashes")]
    pub block_hashes_path: Option<OsString>,

    #[arg(long)]
    pub private_key: Option<B256>,

    #[arg(long)]
    pub external_address: Option<SocketAddr>,

    #[arg(long = "discv5.port", default_value_t = 9009)]
    pub discovery_port: u16,

    #[arg(long = "discv5.enr_cache_capacity", default_value_t = 1000)]
    pub enr_cache_capacity: usize,

    #[arg(long = "discv5.timeout", default_value_t = 5000)]
    pub discv5_timeout_ms: u64,

    #[arg(long = "concurrency.utp", default_value_t = 1000)]
    pub concurrency_utp: usize,

    #[arg(long = "concurrency.census", default_value_t = 100)]
    pub concurrency_census: usize,
    #[arg(long = "concurrency.in", default_value_t = 400)]
    pub concurrency_in: usize,
    #[arg(long = "concurrency.out", default_value_t = 20)]
    pub concurrency_out: usize,

    #[arg(long = "concurrency.per-content", default_value_t = 2)]
    pub concurrency_per_content: usize,
    #[arg(long = "concurrency.per-peer", default_value_t = 1)]
    pub concurrency_per_peer: usize,

    #[arg(long, default_value_t = 1000)]
    pub batch_size: usize,

    #[arg(long, default_value_t = 100)]
    pub max_attempts: usize,

    #[command(subcommand)]
    pub command: Option<FastSyncSubcommands>,
}

#[derive(Subcommand, Clone, Debug, PartialEq, Eq)]
pub enum FastSyncSubcommands {
    FindPeers(FindPeersArgs),
    DirectFetch(DirectFetchArgs),
}

#[derive(ClapArgs, Clone, Debug, PartialEq, Eq)]
pub struct FindPeersArgs {
    #[arg(default_value = "./bin/fast-sync/peer_content.json")]
    pub output_file: OsString,

    #[arg(long, default_value_t = 20)]
    pub content_per_peer: usize,
}

#[derive(ClapArgs, Clone, Debug, PartialEq, Eq)]
pub struct DirectFetchArgs {
    #[arg(default_value = "./bin/fast-sync/peer_content.json")]
    pub input_file: OsString,

    #[arg(long)]
    pub content_per_peer: Option<usize>,
}

pub async fn run() -> anyhow::Result<()> {
    let args = Args::parse();

    // Setup discv5
    let discovery = Discovery::spawn(DiscoveryConfig::from(&args)).await?;

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

    match &args.command {
        None => Sync::run(args, protocol).await?,
        Some(FastSyncSubcommands::FindPeers(find_peers_args)) => {
            commands::find_peers::run(&args, find_peers_args, protocol).await?;
        }
        Some(FastSyncSubcommands::DirectFetch(direct_fetch_args)) => {
            commands::direct_fetch::run(&args, direct_fetch_args, protocol).await?;
        }
    }

    Ok(())
}
