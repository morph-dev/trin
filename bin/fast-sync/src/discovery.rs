use std::{
    collections::HashMap,
    net::{Ipv4Addr, SocketAddr},
    sync::Arc,
    time::Duration,
};

use alloy::primitives::{Bytes, B256};
use anyhow::{anyhow, bail};
use discv5::{
    enr::{CombinedKey, NodeId},
    ConfigBuilder, Discv5, Enr, TalkRequest,
};
use ethportal_api::{
    types::{
        network::Subnetwork,
        portal_wire::MAINNET,
        protocol_versions::{ProtocolVersion, ProtocolVersionList, ENR_PROTOCOL_VERSION_KEY},
    },
    utils::bytes::{hex_decode, hex_encode_upper},
};
use lru::LruCache;
use parking_lot::Mutex;
use tokio::sync::mpsc;
use tracing::{debug, info, warn};

use crate::Args;

pub type EnrTalkRequest = (Enr, TalkRequest);

#[derive(Debug, Clone)]
pub struct DiscoveryConfig {
    private_key: Option<B256>,
    port: u16,
    external_address: Option<SocketAddr>,
    enr_cache_capacity: usize,
    timeout: Duration,
}

impl From<&Args> for DiscoveryConfig {
    fn from(args: &Args) -> Self {
        Self {
            private_key: args.private_key,
            port: args.discovery_port,
            external_address: args.external_address,
            enr_cache_capacity: args.enr_cache_capacity,
            timeout: Duration::from_millis(args.discv5_timeout_ms),
        }
    }
}

pub struct Discovery {
    discv5: Discv5,
    handlers: Arc<Mutex<HashMap<Subnetwork, mpsc::Sender<EnrTalkRequest>>>>,
    enr_cache: Arc<Mutex<LruCache<NodeId, Enr>>>,
    started: bool,
}

impl Discovery {
    pub fn new(config: DiscoveryConfig) -> anyhow::Result<Self> {
        let listen_all_ips = SocketAddr::new(Ipv4Addr::UNSPECIFIED.into(), config.port);
        let external_address = config
            .external_address
            .or_else(|| portalnet::socket::stun_for_external(&listen_all_ips));

        let enr_key = match config.private_key {
            Some(private_key) => {
                CombinedKey::secp256k1_from_bytes(private_key.clone().as_mut_slice())?
            }
            None => {
                let pk = CombinedKey::generate_secp256k1();
                info!(
                    "New private key generated: {}",
                    B256::from_slice(&pk.encode())
                );
                pk
            }
        };
        let mut enr_builder = Enr::builder();

        if let Some(external_address) = external_address {
            enr_builder.ip(external_address.ip());
            enr_builder.udp4(external_address.port());
        } else {
            enr_builder.udp4(config.port);
        }

        enr_builder.seq(
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("Should be able to get seconds since epoch")
                .as_secs(),
        );

        // Add V0 supported versio
        enr_builder.add_value(
            ENR_PROTOCOL_VERSION_KEY,
            &ProtocolVersionList::new(vec![ProtocolVersion::V0]),
        );

        let enr = enr_builder.build(&enr_key).expect("able to build ENR");

        let discv5: Discv5 = Discv5::new(
            enr,
            enr_key,
            ConfigBuilder::new(discv5::ListenConfig::Ipv4 {
                ip: Ipv4Addr::UNSPECIFIED,
                port: config.port,
            })
            .request_timeout(config.timeout)
            .build(),
        )
        .map_err(|err| anyhow!(err))?;

        let enr_cache = Arc::new(Mutex::new(LruCache::new(config.enr_cache_capacity)));

        Ok(Self {
            discv5,
            handlers: Arc::new(Mutex::new(HashMap::new())),
            enr_cache,
            started: false,
        })
    }

    pub async fn start(&mut self) -> anyhow::Result<()> {
        if self.started {
            bail!("Discovery already started")
        }

        self.discv5.start().await.map_err(|err| anyhow!(err))?;
        let mut event_rx = self
            .discv5
            .event_stream()
            .await
            .map_err(|err| anyhow!(err))?;

        let handlers = Arc::clone(&self.handlers);
        let enr_cache = Arc::clone(&self.enr_cache);

        tokio::spawn(async move {
            while let Some(event) = event_rx.recv().await {
                match event {
                    discv5::Event::SessionEstablished(enr, _socket_addr) => {
                        enr_cache.lock().put(enr.node_id(), enr);
                    }
                    discv5::Event::TalkRequest(talk_request) => {
                        let Some(peer) = enr_cache.lock().get(talk_request.node_id()).cloned()
                        else {
                            warn!(
                                node_id = %talk_request.node_id(),
                                "Peer not found for received TalkRequest"
                            );
                            continue;
                        };

                        let protocol_id = hex_encode_upper(talk_request.protocol());
                        let Ok(subnetwork) =
                            MAINNET.get_subnetwork_from_protocol_identifier(&protocol_id)
                        else {
                            info!(protocol_id, "Unsupported protocol",);
                            continue;
                        };
                        let Some(handler) = handlers.lock().get(&subnetwork).cloned() else {
                            warn!(
                                %subnetwork,
                                "Received talk request but handler is not registered"
                            );
                            continue;
                        };

                        if let Err(err) = handler.try_send((peer, talk_request)) {
                            warn!(%err, %protocol_id, "Error handling talk request");
                        }
                    }
                    discv5::Event::SocketUpdated(address) => {
                        warn!("Local ENR address is updated: {address}");
                    }
                    _ => {
                        debug!(?event, "discv5 event not handled");
                    }
                }
            }
        });

        self.started = true;

        let enr = self.local_enr();
        info!(node_id=?enr.node_id(), %enr, "Discovery started");
        info!("{:?}", enr);

        Ok(())
    }

    pub async fn spawn(config: DiscoveryConfig) -> anyhow::Result<Arc<Self>> {
        let mut discovery = Self::new(config)?;
        discovery.start().await?;
        Ok(Arc::new(discovery))
    }

    pub fn register_handler(&self, subnetwork: Subnetwork, handler: mpsc::Sender<EnrTalkRequest>) {
        assert!(
            self.handlers.lock().insert(subnetwork, handler).is_none(),
            "Handler registered twice for subnetwork: {subnetwork}",
        );
    }

    pub fn local_enr(&self) -> Enr {
        self.discv5.local_enr()
    }

    pub fn get_enr_from_cache(&self, node_id: &NodeId) -> Option<Enr> {
        self.enr_cache.lock().get(node_id).cloned()
    }

    pub async fn send_talk_req(
        &self,
        enr: Enr,
        subnetwork: Subnetwork,
        request: Vec<u8>,
    ) -> Result<Bytes, discv5::RequestError> {
        let protocol = MAINNET
            .get_protocol_identifier_from_subnetwork(&subnetwork)
            .expect("should be able to get protocol for subnetwork");
        let protocol = hex_decode(&protocol).expect("should decode protocol");
        self.discv5
            .talk_req(enr, protocol, request)
            .await
            .map(Bytes::from)
    }
}
