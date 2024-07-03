use std::{fmt, hash::Hash};

use serde::{Deserialize, Deserializer, Serialize, Serializer};
use sha2::{Digest as Sha2Digest, Sha256};
use ssz::{Decode, DecodeError, Encode};
use ssz_derive::{Decode, Encode};
use verkle_core::{Point, Stem};

use crate::{ContentKeyError, OverlayContentKey};

// Prefixes for the different types of verkle content keys:
// https://github.com/ethereum/portal-network-specs/blob/5dcdaee84b9476845ad7900befbfff46ac4f00f2/verkle/verkle-state-network.md
const BUNDLE_KEY_PREFIX: u8 = 0x30;
const BRANCH_FRAGMENT_KEY_PREFIX: u8 = 0x31;
const LEAF_FRAGMENT_KEY_PREFIX: u8 = 0x32;

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum VerkleContentKey {
    // The branch-bundle or leaf-bundle node key.
    Bundle(Point),
    // The branch fragment node key.
    BranchFragment(Point),
    // The leaf fragment node key.
    LeafFragment(LeafFragmentKey),
}

impl OverlayContentKey for VerkleContentKey {
    fn content_id(&self) -> [u8; 32] {
        let mut sha256 = Sha256::new();
        sha256.update(self.to_bytes());
        sha256.finalize().into()
    }

    fn to_bytes(&self) -> Vec<u8> {
        let mut bytes: Vec<u8> = vec![];
        match self {
            VerkleContentKey::Bundle(commitment) => {
                bytes.push(BUNDLE_KEY_PREFIX);
                bytes.extend(commitment.as_ssz_bytes());
            }
            VerkleContentKey::BranchFragment(commitment) => {
                bytes.push(BRANCH_FRAGMENT_KEY_PREFIX);
                bytes.extend(commitment.as_ssz_bytes());
            }
            VerkleContentKey::LeafFragment(leaf_fragment_key) => {
                bytes.push(LEAF_FRAGMENT_KEY_PREFIX);
                bytes.extend(leaf_fragment_key.as_ssz_bytes());
            }
        }
        bytes
    }
}

impl TryFrom<Vec<u8>> for VerkleContentKey {
    type Error = ContentKeyError;

    fn try_from(value: Vec<u8>) -> Result<Self, Self::Error> {
        let result = match value.split_first() {
            Some((&BUNDLE_KEY_PREFIX, key)) => Point::from_ssz_bytes(key).map(Self::Bundle),
            Some((&BRANCH_FRAGMENT_KEY_PREFIX, key)) => {
                Point::from_ssz_bytes(key).map(Self::BranchFragment)
            }
            Some((&LEAF_FRAGMENT_KEY_PREFIX, key)) => {
                LeafFragmentKey::from_ssz_bytes(key).map(Self::LeafFragment)
            }
            Some((&selector, _)) => Err(DecodeError::UnionSelectorInvalid(selector)),
            None => Err(DecodeError::InvalidLengthPrefix {
                len: value.len(),
                expected: 1,
            }),
        };
        result.map_err(|e| ContentKeyError::from_decode_error(e, value))
    }
}

impl From<VerkleContentKey> for Vec<u8> {
    fn from(val: VerkleContentKey) -> Self {
        val.to_bytes()
    }
}

impl From<&VerkleContentKey> for Vec<u8> {
    fn from(val: &VerkleContentKey) -> Self {
        val.to_bytes()
    }
}

impl Hash for VerkleContentKey {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        state.write(&self.to_bytes());
    }
}

impl Serialize for VerkleContentKey {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(&self.to_hex())
    }
}

impl<'de> Deserialize<'de> for VerkleContentKey {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;
        Self::from_hex(&s).map_err(serde::de::Error::custom)
    }
}

impl fmt::Display for VerkleContentKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let s = match self {
            VerkleContentKey::Bundle(commitment) => format!("Bundle({commitment:?})"),
            VerkleContentKey::BranchFragment(commitment) => {
                format!("BranchFragment({commitment:?})")
            }
            VerkleContentKey::LeafFragment(leaf_fragment_key) => format!(
                "LeafFragment({}, {:?})",
                &leaf_fragment_key.stem, &leaf_fragment_key.commitment
            ),
        };
        write!(f, "VerkleContentKey-{s}")
    }
}

/// A key for a leaf fragment node.
#[derive(Clone, Debug, Eq, PartialEq, Decode, Encode)]
pub struct LeafFragmentKey {
    /// The stem of the leaf node.
    pub stem: Stem,
    /// The commitment of the Leaf Fragment node.
    pub commitment: Point,
}
