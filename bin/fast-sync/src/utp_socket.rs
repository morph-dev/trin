use std::{
    io::{self, Write},
    sync::Arc,
};

use async_trait::async_trait;
use ethportal_api::types::network::Subnetwork;
use portalnet::discovery::UtpPeer;
use tokio::sync::mpsc;
use tracing::warn;
use utp_rs::{peer::Peer, udp::AsyncUdpSocket};

use crate::discovery::{Discovery, EnrTalkRequest};

pub struct Discovery5UtpSocket {
    discovery: Arc<Discovery>,
    talk_request_rx: mpsc::Receiver<EnrTalkRequest>,
}

impl Discovery5UtpSocket {
    pub fn new(
        discovery: &Arc<Discovery>,
        talk_request_rx: mpsc::Receiver<EnrTalkRequest>,
    ) -> Self {
        Self {
            discovery: Arc::clone(discovery),
            talk_request_rx,
        }
    }
}

#[async_trait]
impl AsyncUdpSocket<UtpPeer> for Discovery5UtpSocket {
    async fn send_to(&mut self, buf: &[u8], peer: &Peer<UtpPeer>) -> io::Result<usize> {
        let discovery = Arc::clone(&self.discovery);
        let Some(peer) = peer
            .peer()
            .map(|peer| peer.0.clone())
            .or_else(|| discovery.get_enr_from_cache(peer.id()))
        else {
            return Err(io::Error::new(io::ErrorKind::Other, "Peer Enr not found"));
        };
        let data = buf.to_vec();
        tokio::spawn(async move {
            if let Err(err) = discovery.send_talk_req(peer, Subnetwork::Utp, data).await {
                warn!(%err, "uTP request failed")
            }
        });
        Ok(buf.len())
    }

    async fn recv_from(&mut self, mut buf: &mut [u8]) -> io::Result<(usize, Peer<UtpPeer>)> {
        match self.talk_request_rx.recv().await {
            Some((peer, talk_req)) => {
                let result = (talk_req.body().len(), Peer::new(UtpPeer(peer)));
                buf.write_all(talk_req.body())?;

                // Respond with empty talk response
                if let Err(err) = talk_req.respond(vec![]) {
                    warn!(%err, "Failed to respond to uTP talk request");
                }
                Ok(result)
            }
            None => Err(io::ErrorKind::NotConnected.into()),
        }
    }
}
