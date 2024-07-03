use portal_verkle_trie::nodes::portal::ssz::nodes::{
    BranchBundleNode, BranchBundleNodeWithProof, BranchFragmentNode, BranchFragmentNodeWithProof,
    LeafBundleNode, LeafBundleNodeWithProof, LeafFragmentNode, LeafFragmentNodeWithProof,
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
