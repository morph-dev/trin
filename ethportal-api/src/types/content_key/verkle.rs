use std::{fmt, hash::Hash};

use serde::{Deserialize, Deserializer, Serialize, Serializer};

use crate::{ContentKeyError, OverlayContentKey};

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum VerkleContentKey {
    InnerRootNode,
    InnerSubNode,
    LeafRootNode,
    LeafSubNode,
}

impl OverlayContentKey for VerkleContentKey {
    fn content_id(&self) -> [u8; 32] {
        todo!()
    }

    fn to_bytes(&self) -> Vec<u8> {
        todo!()
    }
}

impl TryFrom<Vec<u8>> for VerkleContentKey {
    type Error = ContentKeyError;

    fn try_from(_value: Vec<u8>) -> Result<Self, Self::Error> {
        todo!()
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
        write!(f, "VerkleContentKey {{ }}")
    }
}
