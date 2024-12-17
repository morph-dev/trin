use eth_trie::node::Node;
use ethportal_api::types::state_trie::{EncodedTrieNode, TrieProof as EncodedTrieProof};
use revm_primitives::Bytes;

#[derive(Clone, Debug)]
pub struct TrieProof {
    pub path: Vec<u8>,
    pub proof: Vec<Bytes>,
    pub last_node: Node,
    pub last_encoded_node: EncodedTrieNode,
}

impl From<&TrieProof> for EncodedTrieProof {
    fn from(trie_proof: &TrieProof) -> Self {
        EncodedTrieProof::from(
            trie_proof
                .proof
                .iter()
                .map(|bytes| EncodedTrieNode::from(bytes.to_vec()))
                .collect::<Vec<EncodedTrieNode>>(),
        )
    }
}
