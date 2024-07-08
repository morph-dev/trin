use alloy_primitives::B256;
use ethportal_api::{ContentValue, VerkleContentValue};
use thiserror::Error;

// An error that happened while validating verkle state content
#[derive(Debug, Error)]
pub enum VerkleValidationError {
    #[error("Content key has commitment 'Zero'")]
    ZeroCommitment,

    #[error("Wrong commitment: {commitment}, expected {expected_commitment}")]
    _InvalidCommitment {
        commitment: B256,
        expected_commitment: B256,
    },

    #[error("Invalid value type for key type: {content_key_type}. value: {}", value.to_hex())]
    InvalidContentValueType {
        content_key_type: &'static str,
        value: Box<VerkleContentValue>,
    },

    #[error("Error while validating: {0}")]
    _Custom(String),
}
