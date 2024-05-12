use std::sync::Arc;

use ethportal_api::VerkleContentKey;
use tokio::sync::RwLock;
use trin_validation::{
    oracle::HeaderOracle,
    validator::{ValidationResult, Validator},
};

pub struct VerkleValidator {
    _header_oracle: Arc<RwLock<HeaderOracle>>,
}

impl VerkleValidator {
    pub fn new(header_oracle: Arc<RwLock<HeaderOracle>>) -> Self {
        Self {
            _header_oracle: header_oracle,
        }
    }
}

impl Validator<VerkleContentKey> for VerkleValidator {
    async fn validate_content(
        &self,
        _content_key: &VerkleContentKey,
        _content_value: &[u8],
    ) -> anyhow::Result<ValidationResult<VerkleContentKey>> {
        // TODO: Add verkle specific validation
        todo!()
    }
}
