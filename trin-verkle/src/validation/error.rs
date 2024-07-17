use ethportal_api::{ContentValue, VerkleContentValue};
use portal_verkle_primitives::nodes::NodeVerificationError;
use thiserror::Error;

// An error that happened while validating verkle state content
#[derive(Debug, Error)]
pub enum VerkleValidationError {
    #[error("Content key has commitment 'Zero'")]
    ZeroCommitment,

    #[error(
        "Invalid value type for key type: {content_key_type}. value: {}",
        value.to_hex()
    )]
    InvalidContentValueType {
        content_key_type: &'static str,
        value: Box<VerkleContentValue>,
    },

    #[error("Error while verifying node: {0}")]
    NodeVerificationError(#[from] NodeVerificationError),
}
