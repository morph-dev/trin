use std::sync::Arc;

use anyhow::{anyhow, bail};
use ethportal_api::{
    types::content_key::verkle::LeafFragmentKey, ContentValue, VerkleContentKey, VerkleContentValue,
};
use tokio::sync::RwLock;
use trin_validation::{
    oracle::HeaderOracle,
    validator::{ValidationResult, Validator},
};
use verkle_core::{Point, Stem};

use super::error::VerkleValidationError;

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
        content_key: &VerkleContentKey,
        content_value: &[u8],
    ) -> anyhow::Result<ValidationResult<VerkleContentKey>> {
        if content_key.commitment().is_zero() {
            bail!(VerkleValidationError::ZeroCommitment)
        }

        let value = VerkleContentValue::decode(content_value)
            .map_err(|err| anyhow!("Error decoding VerkleContentValue: {err}"))?;

        match content_key {
            VerkleContentKey::Bundle(commitment) => {
                Ok(self.validate_bundle(commitment, value).await?)
            }
            VerkleContentKey::BranchFragment(commitment) => {
                Ok(self.validate_branch_fragment(commitment, value).await?)
            }
            VerkleContentKey::LeafFragment(LeafFragmentKey { stem, commitment }) => {
                Ok(self.validate_leaf_fragment(commitment, stem, value).await?)
            }
        }
    }
}

impl VerkleValidator {
    async fn validate_bundle(
        &self,
        _commitment: &Point,
        value: VerkleContentValue,
    ) -> Result<ValidationResult<VerkleContentKey>, VerkleValidationError> {
        match value {
            VerkleContentValue::BranchBundle(_) => {
                Ok(ValidationResult::new(/* valid_for_storing= */ false))
            }
            VerkleContentValue::BranchBundleWithProof(_) => {
                Ok(ValidationResult::new(/* valid_for_storing= */ true))
            }
            VerkleContentValue::LeafBundle(_) => {
                Ok(ValidationResult::new(/* valid_for_storing= */ false))
            }
            VerkleContentValue::LeafBundleWithProof(_) => {
                Ok(ValidationResult::new(/* valid_for_storing= */ true))
            }
            _ => Err(VerkleValidationError::InvalidContentValueType {
                content_key_type: "Bundle",
                value: value.into(),
            }),
        }
    }

    async fn validate_branch_fragment(
        &self,
        _commitment: &Point,
        value: VerkleContentValue,
    ) -> Result<ValidationResult<VerkleContentKey>, VerkleValidationError> {
        match value {
            VerkleContentValue::BranchFragment(_) => {
                Ok(ValidationResult::new(/* valid_for_storing= */ false))
            }
            VerkleContentValue::BranchFragmentWithProof(_) => {
                Ok(ValidationResult::new(/* valid_for_storing= */ true))
            }
            _ => Err(VerkleValidationError::InvalidContentValueType {
                content_key_type: "BranchFragment",
                value: value.into(),
            }),
        }
    }

    async fn validate_leaf_fragment(
        &self,
        _commitment: &Point,
        _stem: &Stem,
        value: VerkleContentValue,
    ) -> Result<ValidationResult<VerkleContentKey>, VerkleValidationError> {
        match value {
            VerkleContentValue::LeafFragment(_) => {
                Ok(ValidationResult::new(/* valid_for_storing= */ false))
            }
            VerkleContentValue::LeafFragmentWithProof(_) => {
                Ok(ValidationResult::new(/* valid_for_storing= */ true))
            }
            _ => Err(VerkleValidationError::InvalidContentValueType {
                content_key_type: "LeafFragment",
                value: value.into(),
            }),
        }
    }
}
