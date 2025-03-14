use std::sync::Arc;

use clap::{arg, Parser};
use discovery::Discovery;
use ethportal_api::types::network::Subnetwork;
use tokio::sync::mpsc;
use utp_rs::socket::UtpSocket;
use utp_socket::Discovery5UtpSocket;

mod discovery;
mod service;
mod utp_socket;

#[derive(Parser, Debug, Default, PartialEq, Eq, Clone)]
#[command(
    name = "fast-fetch",
    author = "https://github.com/ethereum/trin/graphs/contributors"
)]
pub struct Args {
    #[arg(
        default_value = "0",
        help = "The block number of the first block to fetch."
    )]
    pub first_block: u64,

    #[arg(
        default_value = "10",
        help = "HThe block number of the last bloc to fetch."
    )]
    pub last_block: u64,

    #[arg(
        long = "discovery-port",
        default_value_t = 9009,
        help = "The UDP port to listen on."
    )]
    pub discovery_port: u16,

    #[arg(long, default_value_t = 1000)]
    pub enr_cache_capacity: usize,

    #[arg(long, default_value_t = 1000)]
    pub mpsc_channel_capacity: usize,
}

pub struct FastSync {
    discovery: Arc<Discovery>,
}

impl FastSync {
    pub async fn start(args: Args) -> anyhow::Result<Self> {
        let discovery = Discovery::spawn((&args).into()).await?;

        // Setup uTP
        let (utp_tx, utp_rx) = mpsc::channel(args.mpsc_channel_capacity);
        discovery.register_handler(Subnetwork::Utp, utp_tx);
        let utp_socket = UtpSocket::with_socket(Discovery5UtpSocket::new(&discovery, utp_rx));

        Ok(Self { discovery })
    }
}
