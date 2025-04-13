use std::{ffi::OsString, net::SocketAddr};

use alloy::primitives::B256;
use clap::{arg, Parser};
use sync::Sync;

pub mod census;
pub mod coordinator;
pub mod discovery;
pub mod network;
pub mod protocol;
pub mod sync;
pub mod types;
pub mod utils;
pub mod utp_socket;

#[derive(Parser, Debug, Default, PartialEq, Eq, Clone)]
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

    #[arg(
        long,
        help = "Path to binary file that stores block hashes",
        default_value = ""
    )]
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

    #[arg(long = "concurrency.utp", default_value_t = 1000)]
    pub concurrency_utp: usize,

    #[arg(long = "concurrency.census", default_value_t = 100)]
    pub concurrency_census: usize,
    #[arg(long = "concurrency.in", default_value_t = 400)]
    pub concurrency_in: usize,
    #[arg(long = "concurrency.out", default_value_t = 200)]
    pub concurrency_out: usize,

    #[arg(long = "concurrency.per-content", default_value_t = 1)]
    pub concurrency_per_content: usize,
    #[arg(long = "concurrency.per-peer", default_value_t = 1)]
    pub concurrency_per_peer: usize,

    #[arg(long, default_value_t = 1000)]
    pub batch_size: usize,

    #[arg(long, default_value_t = 100)]
    pub max_attempts: usize,
}

pub async fn run() -> anyhow::Result<()> {
    let args = Args::parse();
    Sync::run(args).await?;
    Ok(())
}
