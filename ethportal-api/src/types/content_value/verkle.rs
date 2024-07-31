use portal_verkle_primitives::{
    portal::{PortalVerkleNode, PortalVerkleNodeWithProof},
    Point,
};
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use ssz::{Decode, Encode};
use ssz_derive::{Decode, Encode};

use crate::{utils::bytes::hex_encode, ContentValue, ContentValueError};

/// Verkle content value.
#[derive(Clone, Debug, PartialEq, Eq, Encode, Decode)]
#[ssz(enum_behaviour = "union")]
#[allow(clippy::large_enum_variant)]
pub enum VerkleContentValue {
    Node(PortalVerkleNode),
    NodeWithProof(PortalVerkleNodeWithProof),
}

impl VerkleContentValue {
    pub fn commitment(&self) -> &Point {
        match &self {
            Self::Node(node) => node.commitment(),
            Self::NodeWithProof(node_with_proof) => node_with_proof.commitment(),
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
