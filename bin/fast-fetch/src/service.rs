use std::{marker::PhantomData, sync::Arc};

use discv5::Enr;
use ethportal_api::types::network::Subnetwork;
use tokio::sync::mpsc;

use crate::discovery::{Discovery, EnrTalkRequest};

pub struct Service<TContentKey, TMetric> {
    subnetwork: Subnetwork,
    query_tx: mpsc::Receiver<EnrTalkRequest>,
    _phantom_content_key: PhantomData<TContentKey>,
    _phantom_metric: PhantomData<TMetric>,
}

impl<TContentKey, TMetric> Service<TContentKey, TMetric> {
    pub async fn spawn(
        discovery: Arc<Discovery>,
        subnetwork: Subnetwork,
        bootnodes: &[Enr],
    ) -> anyhow::Result<()> {
        let (query_rx, query_tx) = mpsc::channel(100);
        discovery.register_handler(subnetwork, query_rx);

        tokio::spawn(async move {});
        todo!()
    }
}
