use serde::{Deserialize, Deserializer, Serialize, Serializer};

use crate::ContentValue;

/// Verkle content value.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum VerkleContentValue {}

impl ContentValue for VerkleContentValue {
    fn encode(&self) -> Vec<u8> {
        todo!()
    }

    fn decode(_buf: &[u8]) -> Result<Self, crate::ContentValueError> {
        todo!()
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
