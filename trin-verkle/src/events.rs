use ethportal_api::types::portal_wire::Message;
use portalnet::events::OverlayRequest;
use std::sync::Arc;
use tokio::sync::mpsc::UnboundedReceiver;
use tracing::{error, warn, Instrument};

use crate::network::VerkleNetwork;

pub struct VerkleEvents {
    pub network: Arc<VerkleNetwork>,
    pub message_rx: UnboundedReceiver<OverlayRequest>,
}

impl VerkleEvents {
    pub async fn start(mut self) {
        loop {
            tokio::select! {
                Some(msg) = self.message_rx.recv() => {
                    self.handle_verkle_message(msg);
                } else => {
                    error!("Verkle event channel closed, shutting down");
                    break;
                }
            }
        }
    }

    /// Handle verkle network OverlayRequest.
    fn handle_verkle_message(&self, msg: OverlayRequest) {
        let network = Arc::clone(&self.network);
        tokio::spawn(async move {
            match msg {
                OverlayRequest::Talk(talk_request) => {
                    Self::handle_talk_request(talk_request, network).await;
                }
                OverlayRequest::Event(event) => {
                    let _ = network.overlay.process_one_event(event).await;
                }
            }
        });
    }

    /// Handle verkle network TALKREQ message.
    async fn handle_talk_request(request: discv5::TalkRequest, network: Arc<VerkleNetwork>) {
        let talk_request_id = request.id().clone();
        let reply = match network
            .overlay
            .process_one_request(&request)
            .instrument(tracing::info_span!("verkle_network", req = %talk_request_id))
            .await
        {
            Ok(response) => Message::from(response).into(),
            Err(error) => {
                error!(
                    error = %error,
                    request.discv5.id = %talk_request_id,
                    "Error processing portal verkle request, responding with empty TALKRESP"
                );
                // Return an empty TALKRESP if there was an error executing the request
                "".into()
            }
        };
        if let Err(error) = request.respond(reply) {
            warn!(error = %error, request.discv5.id = %talk_request_id, "Error responding to TALKREQ");
        }
    }
}
