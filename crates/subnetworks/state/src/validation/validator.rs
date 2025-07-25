use std::sync::Arc;

use alloy::{
    hex::ToHexExt,
    primitives::{keccak256, B256},
};
use ethportal_api::{
    types::content_key::state::{
        AccountTrieNodeKey, ContractBytecodeKey, ContractStorageTrieNodeKey,
    },
    ContentValue, OverlayContentKey, StateContentKey, StateContentValue,
};
use tokio::sync::RwLock;
use tracing::debug;
use trin_validation::{
    oracle::HeaderOracle,
    validator::{ValidationResult, Validator},
};

use super::{
    error::{check_node_hash, StateValidationError},
    trie::{validate_account_state, validate_node_trie_proof},
};

// todo: remove this constant once the history network implements full chain header validation
const DISABLE_LEGACY_HISTORY_HEADER_CHECK: bool = true;

pub struct StateValidator {
    pub header_oracle: Arc<RwLock<HeaderOracle>>,
}

impl Validator<StateContentKey> for StateValidator {
    async fn validate_content(
        &self,
        content_key: &StateContentKey,
        content_value: &[u8],
    ) -> ValidationResult {
        let Ok(content_value) = StateContentValue::decode(content_key, content_value) else {
            debug!(
                content_key = content_key.to_hex(),
                content_value = content_value.encode_hex_with_prefix(),
                "Error decoding StateContentValue",
            );
            return ValidationResult::Invalid(format!(
                "Error decoding StateContentValue for key: {content_key:?}",
            ));
        };

        let validation_result = match content_key {
            StateContentKey::AccountTrieNode(key) => {
                self.validate_account_trie_node(key, content_value).await
            }
            StateContentKey::ContractStorageTrieNode(key) => {
                self.validate_contract_storage_trie_node(key, content_value)
                    .await
            }
            StateContentKey::ContractBytecode(key) => {
                self.validate_contract_bytecode(key, content_value).await
            }
        };
        validation_result.unwrap_or_else(|err| {
            ValidationResult::Invalid(format!("Error validating StateContentValue: {err:?}"))
        })
    }
}

impl StateValidator {
    async fn validate_account_trie_node(
        &self,
        key: &AccountTrieNodeKey,
        value: StateContentValue,
    ) -> Result<ValidationResult, StateValidationError> {
        match value {
            StateContentValue::TrieNode(value) => {
                check_node_hash(&value.node, &key.node_hash)?;
                Ok(ValidationResult::Valid)
            }
            StateContentValue::AccountTrieNodeWithProof(value) => {
                let state_root = match DISABLE_LEGACY_HISTORY_HEADER_CHECK {
                    true => None,
                    false => Some(self.get_state_root(value.block_hash).await?),
                };
                validate_node_trie_proof(state_root, key.node_hash, &key.path, &value.proof)?;

                Ok(ValidationResult::CanonicallyValid)
            }
            _ => Err(StateValidationError::InvalidContentValueType(
                "AccountTrieNodeKey",
            )),
        }
    }

    async fn validate_contract_storage_trie_node(
        &self,
        key: &ContractStorageTrieNodeKey,
        value: StateContentValue,
    ) -> Result<ValidationResult, StateValidationError> {
        match value {
            StateContentValue::TrieNode(value) => {
                check_node_hash(&value.node, &key.node_hash)?;
                Ok(ValidationResult::Valid)
            }
            StateContentValue::ContractStorageTrieNodeWithProof(value) => {
                let state_root = match DISABLE_LEGACY_HISTORY_HEADER_CHECK {
                    true => None,
                    false => Some(self.get_state_root(value.block_hash).await?),
                };
                let account_state =
                    validate_account_state(state_root, &key.address_hash, &value.account_proof)?;
                validate_node_trie_proof(
                    Some(account_state.storage_root),
                    key.node_hash,
                    &key.path,
                    &value.storage_proof,
                )?;

                Ok(ValidationResult::CanonicallyValid)
            }
            _ => Err(StateValidationError::InvalidContentValueType(
                "ContractStorageTrieNodeKey",
            )),
        }
    }

    async fn validate_contract_bytecode(
        &self,
        key: &ContractBytecodeKey,
        value: StateContentValue,
    ) -> Result<ValidationResult, StateValidationError> {
        match value {
            StateContentValue::ContractBytecode(value) => {
                let bytecode_hash = keccak256(&value.code[..]);
                if bytecode_hash == key.code_hash {
                    Ok(ValidationResult::Valid)
                } else {
                    Err(StateValidationError::InvalidBytecodeHash {
                        bytecode_hash,
                        expected_bytecode_hash: key.code_hash,
                    })
                }
            }
            StateContentValue::ContractBytecodeWithProof(value) => {
                let bytecode_hash = keccak256(&value.code[..]);
                if bytecode_hash != key.code_hash {
                    return Err(StateValidationError::InvalidBytecodeHash {
                        bytecode_hash,
                        expected_bytecode_hash: key.code_hash,
                    });
                }

                let state_root = match DISABLE_LEGACY_HISTORY_HEADER_CHECK {
                    true => None,
                    false => Some(self.get_state_root(value.block_hash).await?),
                };
                let account_state =
                    validate_account_state(state_root, &key.address_hash, &value.account_proof)?;
                if account_state.code_hash == key.code_hash {
                    Ok(ValidationResult::CanonicallyValid)
                } else {
                    Err(StateValidationError::InvalidBytecodeHash {
                        bytecode_hash,
                        expected_bytecode_hash: account_state.code_hash,
                    })
                }
            }
            _ => Err(StateValidationError::InvalidContentValueType(
                "ContractBytecodeKey",
            )),
        }
    }

    async fn get_state_root(&self, block_hash: B256) -> Result<B256, StateValidationError> {
        let header_oracle = self.header_oracle.read().await;
        let header = header_oracle
            .recursive_find_header_by_hash_with_proof(block_hash)
            .await?;
        Ok(header.header.state_root)
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use alloy::{consensus::Header, primitives::Bytes, rlp::Decodable};
    use anyhow::Result;
    use ethportal_api::{
        types::{
            execution::header_with_proof::{BlockHeaderProof, HeaderWithProof},
            jsonrpc::{endpoints::LegacyHistoryEndpoint, json_rpc_mock::MockJsonRpcBuilder},
            portal::GetContentInfo,
        },
        LegacyHistoryContentKey, LegacyHistoryContentValue, OverlayContentKey,
    };
    use serde::Deserialize;
    use serde_yaml::Value;
    use trin_utils::submodules::read_portal_spec_tests_file;

    use super::*;

    const TEST_DIRECTORY: &str = "tests/mainnet/state/validation";

    fn create_validator() -> StateValidator {
        let header_oracle = Arc::new(RwLock::new(HeaderOracle::new()));
        StateValidator { header_oracle }
    }

    fn create_validator_with_header(header: Header) -> StateValidator {
        let legacy_history_content_value =
            LegacyHistoryContentValue::BlockHeaderWithProof(HeaderWithProof {
                header: header.clone(),
                proof: BlockHeaderProof::HistoricalHashes(Default::default()),
            });
        let legacy_history_jsonrpc_tx = MockJsonRpcBuilder::new()
            .with_response(
                LegacyHistoryEndpoint::GetContent(
                    LegacyHistoryContentKey::new_block_header_by_hash(header.hash_slow()),
                ),
                GetContentInfo {
                    content: legacy_history_content_value.encode(),
                    utp_transfer: false,
                },
            )
            .or_fail();

        let mut header_oracle = HeaderOracle::new();
        header_oracle.legacy_history_jsonrpc_tx = Some(legacy_history_jsonrpc_tx);

        StateValidator {
            header_oracle: Arc::new(RwLock::new(header_oracle)),
        }
    }

    fn read_yaml_file_as_sequence(filename: &str) -> Vec<Value> {
        let yaml_file = read_portal_spec_tests_file(PathBuf::from(TEST_DIRECTORY).join(filename))
            .expect("to read yaml file");

        serde_yaml::from_str::<Value>(&yaml_file)
            .expect("to decode yaml file")
            .as_sequence()
            .expect("to be sequence")
            .clone()
    }

    #[tokio::test]
    async fn account_trie_node_retrieval() -> Result<()> {
        let test_cases = read_yaml_file_as_sequence("account_trie_node.yaml");
        for test_case in test_cases {
            let content_key = StateContentKey::deserialize(&test_case["content_key"])?;
            let content_value = Bytes::deserialize(&test_case["content_value_retrieval"])?;

            let validation_result = create_validator()
                .validate_content(&content_key, content_value.as_ref())
                .await;
            assert_eq!(
                validation_result,
                ValidationResult::Valid,
                "testing content_key: {}",
                content_key.to_hex()
            );
        }

        Ok(())
    }

    #[tokio::test]
    async fn account_trie_node_offer() -> Result<()> {
        let test_cases = read_yaml_file_as_sequence("account_trie_node.yaml");
        for test_case in test_cases {
            let content_key = StateContentKey::deserialize(&test_case["content_key"])?;
            let content_value = Bytes::deserialize(&test_case["content_value_offer"])?;
            let header = Bytes::deserialize(&test_case["block_header"])?;

            let validation_result =
                create_validator_with_header(Header::decode(&mut header.as_ref())?)
                    .validate_content(&content_key, content_value.as_ref())
                    .await;

            assert_eq!(
                validation_result,
                ValidationResult::CanonicallyValid,
                "testing content_key: {}",
                content_key.to_hex()
            );
        }

        Ok(())
    }

    #[tokio::test]
    async fn contract_storage_trie_node_retrieval() -> Result<()> {
        let test_cases = read_yaml_file_as_sequence("contract_storage_trie_node.yaml");
        for test_case in test_cases {
            let content_key = StateContentKey::deserialize(&test_case["content_key"])?;
            let content_value = Bytes::deserialize(&test_case["content_value_retrieval"])?;

            let validation_result = create_validator()
                .validate_content(&content_key, content_value.as_ref())
                .await;
            assert_eq!(
                validation_result,
                ValidationResult::Valid,
                "testing content_key: {}",
                content_key.to_hex()
            );
        }

        Ok(())
    }

    #[tokio::test]
    async fn contract_storage_trie_node_offer() -> Result<()> {
        let test_cases = read_yaml_file_as_sequence("contract_storage_trie_node.yaml");
        for test_case in test_cases {
            let content_key = StateContentKey::deserialize(&test_case["content_key"])?;
            let content_value = Bytes::deserialize(&test_case["content_value_offer"])?;
            let header = Bytes::deserialize(&test_case["block_header"])?;

            let validation_result =
                create_validator_with_header(Header::decode(&mut header.as_ref())?)
                    .validate_content(&content_key, content_value.as_ref())
                    .await;

            assert_eq!(
                validation_result,
                ValidationResult::CanonicallyValid,
                "testing content_key: {}",
                content_key.to_hex()
            );
        }

        Ok(())
    }

    #[tokio::test]
    async fn contract_bytecode_retrieval() -> Result<()> {
        let test_cases = read_yaml_file_as_sequence("contract_bytecode.yaml");
        for test_case in test_cases {
            let content_key = StateContentKey::deserialize(&test_case["content_key"])?;
            let content_value = Bytes::deserialize(&test_case["content_value_retrieval"])?;

            let validation_result = create_validator()
                .validate_content(&content_key, content_value.as_ref())
                .await;
            assert_eq!(
                validation_result,
                ValidationResult::Valid,
                "testing content_key: {}",
                content_key.to_hex()
            );
        }

        Ok(())
    }

    #[tokio::test]
    async fn contract_bytecode_offer() -> Result<()> {
        let test_cases = read_yaml_file_as_sequence("contract_bytecode.yaml");
        for test_case in test_cases {
            let content_key = StateContentKey::deserialize(&test_case["content_key"])?;
            let content_value = Bytes::deserialize(&test_case["content_value_offer"])?;
            let header = Bytes::deserialize(&test_case["block_header"])?;

            let validation_result =
                create_validator_with_header(Header::decode(&mut header.as_ref())?)
                    .validate_content(&content_key, content_value.as_ref())
                    .await;
            assert_eq!(
                validation_result,
                ValidationResult::CanonicallyValid,
                "testing content_key: {}",
                content_key.to_hex()
            );
        }

        Ok(())
    }
}
