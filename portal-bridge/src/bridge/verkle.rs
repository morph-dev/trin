use std::{ops::Range, path::PathBuf, sync::Arc};

use ethportal_api::jsonrpsee::http_client::HttpClient;
use tokio::sync::Semaphore;
use tracing::info;
use trin_metrics::bridge::BridgeMetricsReporter;
use trin_validation::oracle::HeaderOracle;

use crate::types::mode::{BridgeMode, ModeType};

pub struct VerkleBridge {
    pub mode: BridgeMode,
    pub portal_client: HttpClient,
    pub header_oracle: HeaderOracle,
    pub epoch_acc_path: PathBuf,
    pub metrics: BridgeMetricsReporter,
    pub gossip_limit_semaphore: Arc<Semaphore>,
}

impl VerkleBridge {
    pub async fn new(
        mode: BridgeMode,
        portal_client: HttpClient,
        header_oracle: HeaderOracle,
        epoch_acc_path: PathBuf,
        gossip_limit: usize,
    ) -> anyhow::Result<Self> {
        let metrics = BridgeMetricsReporter::new("verkle".to_string(), &format!("{mode:?}"));

        // We are using a semaphore to limit the amount of active gossip transfers to make sure
        // we don't overwhelm the trin client
        let gossip_limit_semaphore = Arc::new(Semaphore::new(gossip_limit));
        Ok(Self {
            mode,
            portal_client,
            header_oracle,
            epoch_acc_path,
            metrics,
            gossip_limit_semaphore,
        })
    }

    pub async fn launch(&self) {
        info!("Launching verkle bridge: {:?}", self.mode);
        match self.mode.clone() {
            BridgeMode::Backfill(ModeType::BlockRange(start, end)) => {
                let block_range = start..end + 1;
                self.launch_verkle(block_range)
                    .await
                    .expect("Verkle bridge failed.")
            }
            _ => panic!("Verkle bridge only supports 'backfill:rXX-YY' mode"),
        }
        info!("Bridge mode: {:?} complete.", self.mode);
    }

    async fn launch_verkle(&self, block_range: Range<u64>) -> anyhow::Result<()> {
        info!("Gossiping verkle data from block range: {block_range:?}");
        todo!()
    }
}
