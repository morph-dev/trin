use serde::{Deserialize, Serialize};
use ssz_derive::{Decode, Encode};
use ssz_types::{typenum, VariableList};
use tree_hash_derive::TreeHash;

use super::{state_diff::StemStateDiff, verkle_proof::VerkleProof};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Encode, Decode, TreeHash)]
pub struct ExecutionWitness {
    #[serde(alias = "stateDiff")]
    pub state_diff: VariableList<StemStateDiff, typenum::U65536>,
    #[serde(alias = "verkleProof")]
    pub verkle_proof: VerkleProof,
}
