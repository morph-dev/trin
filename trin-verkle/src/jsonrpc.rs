use ethportal_api::types::jsonrpc::request::VerkleJsonRpcRequest;
use tokio::sync::mpsc;

/// Handles Verkle network JSON-RPC requests
pub struct VerkleRequestHandler {
    // pub network: Arc<VerkleNetwork>,
    pub verkle_rx: mpsc::UnboundedReceiver<VerkleJsonRpcRequest>,
}

impl VerkleRequestHandler {
    pub async fn handle_client_queries(mut self) {
        while let Some(_request) = self.verkle_rx.recv().await {
            todo!()
        }
    }
}
