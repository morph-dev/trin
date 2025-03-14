use std::{net::SocketAddr, sync::Arc};

use alloy::primitives::B256;
use clap::{arg, Parser};
use discovery::Discovery;
use ethportal_api::{
    types::{distance::XorMetric, network::Subnetwork},
    HistoryContentKey,
};
use portalnet::discovery::UtpPeer;
use service::{Service, ServiceConfig};
use tokio::sync::mpsc;
use utp_rs::socket::UtpSocket;
use utp_socket::Discovery5UtpSocket;

pub mod discovery;
pub mod service;
pub mod utp_socket;

#[derive(Parser, Debug, Default, PartialEq, Eq, Clone)]
#[command(
    name = "fast-fetch",
    author = "https://github.com/ethereum/trin/graphs/contributors"
)]
pub struct Args {
    #[arg(help = "The block number of the first block to fetch.")]
    pub first_block: u64,

    #[arg(help = "The block number of the last bloc to fetch.")]
    pub last_block: u64,

    #[arg(long)]
    pub private_key: Option<B256>,

    #[arg(long = "discovery-port", default_value_t = 9009)]
    pub discovery_port: u16,

    #[arg(long)]
    pub external_address: Option<SocketAddr>,

    #[arg(long, default_value_t = 1000)]
    pub enr_cache_capacity: usize,

    #[arg(long, default_value_t = 1000)]
    pub mpsc_channel_capacity: usize,
}

pub struct FastSync {
    _discovery: Arc<Discovery>,
    _utp_socket: Arc<UtpSocket<UtpPeer>>,
    _history: Arc<Service<HistoryContentKey, XorMetric>>,
}

impl FastSync {
    pub async fn start(args: Args) -> anyhow::Result<Self> {
        // Setup discv5
        let discovery = Discovery::spawn((&args).into()).await?;

        // Setup uTP
        let (utp_tx, utp_rx) = mpsc::channel(args.mpsc_channel_capacity);
        discovery.register_handler(Subnetwork::Utp, utp_tx);
        let utp_socket = Arc::new(UtpSocket::with_socket(Discovery5UtpSocket::new(
            &discovery, utp_rx,
        )));

        // Setup History Subnetwork
        let history = Service::<HistoryContentKey, XorMetric>::spawn(
            Subnetwork::History,
            discovery.clone(),
            utp_socket.clone(),
            ServiceConfig {
                incoming_talk_request_capacity: args.mpsc_channel_capacity,
                outgoing_talk_request_capacity: args.mpsc_channel_capacity,
            },
        )?;

        Ok(Self {
            _discovery: discovery,
            _utp_socket: utp_socket,
            _history: history,
        })
    }
}
