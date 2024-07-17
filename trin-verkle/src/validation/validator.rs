use std::sync::Arc;

use alloy_primitives::B256;
use anyhow::{anyhow, bail};
use ethportal_api::{ContentValue, VerkleContentKey, VerkleContentValue};
use portal_verkle_primitives::nodes::{PortalVerkleNode, PortalVerkleNodeWithProof};
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

        match &value {
            VerkleContentValue::Node(node) => {
                match content_key {
                    VerkleContentKey::Bundle(commitment) => match node {
                        PortalVerkleNode::BranchBundle(node) => node.verify(commitment)?,
                        PortalVerkleNode::LeafBundle(node) => node.verify(commitment)?,
                        _ => bail!(VerkleValidationError::InvalidContentValueType {
                            content_key_type: "Bundle",
                            value: Box::new(value),
                        }),
                    },
                    VerkleContentKey::BranchFragment(commitment) => match node {
                        PortalVerkleNode::BranchFragment(node) => node.verify(commitment)?,
                        _ => bail!(VerkleValidationError::InvalidContentValueType {
                            content_key_type: "BranchFragment",
                            value: Box::new(value),
                        }),
                    },
                    VerkleContentKey::LeafFragment(leaf_fragment_key) => match node {
                        PortalVerkleNode::LeafFragment(node) => {
                            node.verify(&leaf_fragment_key.commitment)?
                        }
                        _ => bail!(VerkleValidationError::InvalidContentValueType {
                            content_key_type: "LeafFragment",
                            value: Box::new(value),
                        }),
                    },
                }
                Ok(ValidationResult::new(/* valid_for_storing= */ false))
            }
            VerkleContentValue::NodeWithProof(node_with_proof) => {
                let state_root = self.get_state_root(&node_with_proof.block_hash()).await;
                match content_key {
                    VerkleContentKey::Bundle(commitment) => match node_with_proof {
                        PortalVerkleNodeWithProof::BranchBundle(node_with_proof) => node_with_proof
                            .verify(
                                commitment,
                                &self.get_state_root(&node_with_proof.block_hash).await,
                            )?,
                        PortalVerkleNodeWithProof::LeafBundle(node_with_proof) => {
                            node_with_proof.verify(commitment, &state_root)?
                        }
                        _ => bail!(VerkleValidationError::InvalidContentValueType {
                            content_key_type: "Bundle",
                            value: Box::new(value),
                        }),
                    },
                    VerkleContentKey::BranchFragment(commitment) => match node_with_proof {
                        PortalVerkleNodeWithProof::BranchFragment(node_with_proof) => {
                            node_with_proof.verify(commitment, &state_root)?
                        }
                        _ => bail!(VerkleValidationError::InvalidContentValueType {
                            content_key_type: "BranchFragment",
                            value: Box::new(value),
                        }),
                    },
                    VerkleContentKey::LeafFragment(leaf_fragment_key) => match node_with_proof {
                        PortalVerkleNodeWithProof::LeafFragment(node_with_proof) => node_with_proof
                            .verify(
                                &leaf_fragment_key.commitment,
                                &state_root,
                                &leaf_fragment_key.stem,
                            )?,
                        _ => bail!(VerkleValidationError::InvalidContentValueType {
                            content_key_type: "LeafFragment",
                            value: Box::new(value),
                        }),
                    },
                }
                Ok(ValidationResult::new(/* valid_for_storing= */ true))
            }
        }
    }
}

impl VerkleValidator {
    async fn get_state_root(&self, _block_hash: &B256) -> B256 {
        // TODO: Implement using header_oracle
        debug!("Fetching state root is not yet implemented");
        B256::ZERO
    }
}
