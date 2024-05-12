use ethportal_api::{
    types::{distance::XorMetric, portal_wire::ProtocolId},
    VerkleContentKey,
};
use parking_lot::RwLock as PLRwLock;
use portalnet::{
    config::PortalnetConfig,
    discovery::{Discovery, UtpEnr},
    overlay::{config::OverlayConfig, protocol::OverlayProtocol},
};
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::debug;
use trin_storage::PortalStorageConfig;
use trin_validation::oracle::HeaderOracle;
use utp_rs::socket::UtpSocket;

use crate::{storage::VerkleStorage, validator::VerkleValidator};

/// Verkle network layer on top of the overlay protocol. Encapsulates Verkle network specific data
/// and logic.
#[derive(Clone)]
pub struct VerkleNetwork {
    pub overlay: Arc<OverlayProtocol<VerkleContentKey, XorMetric, VerkleValidator, VerkleStorage>>,
}

impl VerkleNetwork {
    /// Poke is disabled for Verkle network because Offer/Accept and Find/Found Content are
    /// different, and recipient of the poke wouldn't be able to verify that content is canonical.
    const DISABLE_POKE: bool = true;

    pub async fn new(
        discovery: Arc<Discovery>,
        utp_socket: Arc<UtpSocket<UtpEnr>>,
        storage_config: PortalStorageConfig,
        portal_config: PortalnetConfig,
        header_oracle: Arc<RwLock<HeaderOracle>>,
    ) -> anyhow::Result<Self> {
        if !portal_config.disable_poke {
            debug!("Poke is not supported by the Verkle Network")
        }
        let config = OverlayConfig {
            bootnode_enrs: portal_config.bootnodes.into(),
            disable_poke: Self::DISABLE_POKE,
            utp_transfer_limit: portal_config.utp_transfer_limit,
            ..Default::default()
        };
        let storage = Arc::new(PLRwLock::new(VerkleStorage::new(storage_config)?));
        let validator = Arc::new(VerkleValidator::new(header_oracle));
        let overlay = OverlayProtocol::new(
            config,
            discovery,
            utp_socket,
            storage,
            ProtocolId::Verkle,
            validator,
        )
        .await;

        Ok(Self {
            overlay: Arc::new(overlay),
        })
    }
}
