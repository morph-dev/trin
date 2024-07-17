use std::sync::Arc;

use alloy_primitives::B256;
use anyhow::{anyhow, bail};
use ethportal_api::{
    types::content_key::verkle::LeafFragmentKey, ContentValue, VerkleContentKey, VerkleContentValue,
};
use portal_verkle_primitives::{Point, Stem};
use tokio::sync::RwLock;
use tracing::debug;
use trin_validation::{
    oracle::HeaderOracle,
    validator::{ValidationResult, Validator},
};

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

        if value.commitment() != content_key.commitment() {
            bail!(VerkleValidationError::InvalidCommitment {
                commitment: value.commitment().into(),
                expected_commitment: content_key.commitment().into(),
            })
        }

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
        commitment: &Point,
        value: VerkleContentValue,
    ) -> Result<ValidationResult<VerkleContentKey>, VerkleValidationError> {
        // NOTE: we already verified that commitments match
        match value {
            VerkleContentValue::BranchBundle(node) => {
                node.verify_bundle_proof()?;
                Ok(ValidationResult::new(/* valid_for_storing= */ false))
            }
            VerkleContentValue::BranchBundleWithProof(node_with_proof) => {
                node_with_proof.verify(
                    commitment,
                    &self.get_state_root(&node_with_proof.block_hash).await,
                )?;
                Ok(ValidationResult::new(/* valid_for_storing= */ true))
            }
            VerkleContentValue::LeafBundle(node) => {
                node.verify_bundle_proof()?;
                Ok(ValidationResult::new(/* valid_for_storing= */ false))
            }
            VerkleContentValue::LeafBundleWithProof(node_with_proof) => {
                node_with_proof.verify(
                    commitment,
                    &self.get_state_root(&node_with_proof.block_hash).await,
                )?;
                Ok(ValidationResult::new(/* valid_for_storing= */ true))
            }
            _ => Err(VerkleValidationError::InvalidContentValueType {
                content_key_type: "Bundle",
                value_hex: value.to_hex(),
            }),
        }
    }

    async fn validate_branch_fragment(
        &self,
        commitment: &Point,
        value: VerkleContentValue,
    ) -> Result<ValidationResult<VerkleContentKey>, VerkleValidationError> {
        // NOTE: we already verified that commitments match
        match value {
            VerkleContentValue::BranchFragment(_) => {
                Ok(ValidationResult::new(/* valid_for_storing= */ false))
            }
            VerkleContentValue::BranchFragmentWithProof(node_with_proof) => {
                node_with_proof.verify(
                    commitment,
                    &self.get_state_root(&node_with_proof.block_hash).await,
                )?;
                Ok(ValidationResult::new(/* valid_for_storing= */ true))
            }
            _ => Err(VerkleValidationError::InvalidContentValueType {
                content_key_type: "BranchFragment",
                value_hex: value.to_hex(),
            }),
        }
    }

    async fn validate_leaf_fragment(
        &self,
        commitment: &Point,
        stem: &Stem,
        value: VerkleContentValue,
    ) -> Result<ValidationResult<VerkleContentKey>, VerkleValidationError> {
        // NOTE: we already verified that commitments match
        match value {
            VerkleContentValue::LeafFragment(_) => {
                Ok(ValidationResult::new(/* valid_for_storing= */ false))
            }
            VerkleContentValue::LeafFragmentWithProof(node_with_proof) => {
                node_with_proof.verify(
                    commitment,
                    &self.get_state_root(&node_with_proof.block_hash).await,
                    stem,
                )?;
                Ok(ValidationResult::new(/* valid_for_storing= */ true))
            }
            _ => Err(VerkleValidationError::InvalidContentValueType {
                content_key_type: "LeafFragment",
                value_hex: value.to_hex(),
            }),
        }
    }

    async fn get_state_root(&self, _block_hash: &B256) -> B256 {
        // TODO: Implement using header_oracle
        debug!("Fetching state root is not yet implemented");
        B256::ZERO
    }
}
