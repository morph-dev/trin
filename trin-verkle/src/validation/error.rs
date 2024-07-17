use alloy_primitives::B256;
use portal_verkle_primitives::nodes::NodeVerificationError;
use thiserror::Error;

// An error that happened while validating verkle state content
#[derive(Debug, Error)]
pub enum VerkleValidationError {
    #[error("Content key has commitment 'Zero'")]
    ZeroCommitment,

    #[error("Wrong commitment: {commitment:?}, expected {expected_commitment:?}")]
    InvalidCommitment {
        commitment: B256,
        expected_commitment: B256,
    },

    #[error(
        "Invalid value type for key type: {content_key_type}. value: {}",
        value_hex
    )]
    InvalidContentValueType {
        content_key_type: &'static str,
        value_hex: String,
    },

    #[error("Error while verifying node: {0}")]
    NodeVerificationError(#[from] NodeVerificationError),
}
