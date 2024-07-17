use portal_verkle_primitives::{
    nodes::{
        BranchBundleNode, BranchBundleNodeWithProof, BranchFragmentNode,
        BranchFragmentNodeWithProof, LeafBundleNode, LeafBundleNodeWithProof, LeafFragmentNode,
        LeafFragmentNodeWithProof,
    },
    Point,
};
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use ssz::{Decode, Encode};
use ssz_derive::{Decode, Encode};

use crate::{utils::bytes::hex_encode, ContentValue, ContentValueError};

/// Verkle content value.
#[derive(Clone, Debug, PartialEq, Eq, Encode, Decode)]
#[ssz(enum_behaviour = "union")]
pub enum VerkleContentValue {
    BranchBundle(BranchBundleNode),
    BranchBundleWithProof(BranchBundleNodeWithProof),
    BranchFragment(BranchFragmentNode),
    BranchFragmentWithProof(BranchFragmentNodeWithProof),
    LeafBundle(LeafBundleNode),
    LeafBundleWithProof(LeafBundleNodeWithProof),
    LeafFragment(LeafFragmentNode),
    LeafFragmentWithProof(LeafFragmentNodeWithProof),
}

impl VerkleContentValue {
    pub fn has_proof(&self) -> bool {
        match &self {
            VerkleContentValue::BranchBundle(_)
            | VerkleContentValue::BranchFragment(_)
            | VerkleContentValue::LeafBundle(_)
            | VerkleContentValue::LeafFragment(_) => false,

            VerkleContentValue::BranchBundleWithProof(_)
            | VerkleContentValue::BranchFragmentWithProof(_)
            | VerkleContentValue::LeafBundleWithProof(_)
            | VerkleContentValue::LeafFragmentWithProof(_) => true,
        }
    }

    pub fn commitment(&self) -> &Point {
        match &self {
            VerkleContentValue::BranchBundle(node) => node.commitment(),
            VerkleContentValue::BranchBundleWithProof(node_with_proof) => {
                node_with_proof.node.commitment()
            }
            VerkleContentValue::BranchFragment(node) => node.commitment(),
            VerkleContentValue::BranchFragmentWithProof(node_with_proof) => {
                node_with_proof.node.commitment()
            }
            VerkleContentValue::LeafBundle(node) => node.commitment(),
            VerkleContentValue::LeafBundleWithProof(node_with_proof) => {
                node_with_proof.node.commitment()
            }
            VerkleContentValue::LeafFragment(node) => node.commitment(),
            VerkleContentValue::LeafFragmentWithProof(node_with_proof) => {
                node_with_proof.node.commitment()
            }
        }
    }

    pub fn into_storing_value(self) -> Self {
        match self {
            VerkleContentValue::BranchBundle(_)
            | VerkleContentValue::BranchFragment(_)
            | VerkleContentValue::LeafBundle(_)
            | VerkleContentValue::LeafFragment(_) => self,

            VerkleContentValue::BranchBundleWithProof(node_with_proof) => {
                Self::BranchBundle(node_with_proof.node)
            }
            VerkleContentValue::BranchFragmentWithProof(node_with_proof) => {
                Self::BranchFragment(node_with_proof.node)
            }
            VerkleContentValue::LeafBundleWithProof(node_with_proof) => {
                Self::LeafBundle(node_with_proof.node)
            }
            VerkleContentValue::LeafFragmentWithProof(node_with_proof) => {
                Self::LeafFragment(node_with_proof.node)
            }
        }
    }
}

impl ContentValue for VerkleContentValue {
    fn encode(&self) -> Vec<u8> {
        self.as_ssz_bytes()
    }

    fn decode(buf: &[u8]) -> Result<Self, ContentValueError> {
        Self::from_ssz_bytes(buf).map_err(|err| ContentValueError::DecodeSsz {
            decode_error: err,
            input: hex_encode(buf),
        })
    }
}

impl Serialize for VerkleContentValue {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(&self.to_hex())
    }
}

impl<'de> Deserialize<'de> for VerkleContentValue {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;
        Self::from_hex(&s).map_err(serde::de::Error::custom)
    }
}
