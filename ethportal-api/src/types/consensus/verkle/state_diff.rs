use alloy_primitives::{B256, U8};
use portal_verkle_primitives::{Stem, TrieValue};
use serde::{Deserialize, Serialize};
use ssz_derive::{Decode, Encode};
use ssz_types::{typenum, FixedVector, VariableList};
use tree_hash::{Hash256, PackedEncoding, TreeHash, TreeHashType};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Encode, Decode)]
#[serde(deny_unknown_fields)]
pub struct SuffixStateDiff {
    pub suffix: U8,
    #[serde(alias = "currentValue")]
    pub current_value: Option<TrieValue>,
    #[serde(alias = "newValue")]
    pub new_value: Option<TrieValue>,
}

impl TreeHash for SuffixStateDiff {
    fn tree_hash_type() -> TreeHashType {
        TreeHashType::Container
    }

    fn tree_hash_packed_encoding(&self) -> PackedEncoding {
        unreachable!("SuffixStateDiff should never be packed.")
    }

    fn tree_hash_packing_factor() -> usize {
        unreachable!("SuffixStateDiff should never be packed.")
    }

    fn tree_hash_root(&self) -> Hash256 {
        let mut flag = B256::ZERO;
        flag[0] = self.suffix.byte(0);
        flag[1] = self.current_value.is_some() as u8;
        flag[2] = self.new_value.is_some() as u8;

        FixedVector::<B256, typenum::U3>::new(vec![
            flag,
            self.current_value.as_ref().unwrap_or(&TrieValue::ZERO).0,
            self.new_value.as_ref().unwrap_or(&TrieValue::ZERO).0,
        ])
        .expect("suffix state diff to fixed B256 vector should succeed")
        .tree_hash_root()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Encode, Decode)]
#[serde(deny_unknown_fields)]
pub struct StemStateDiff {
    pub stem: Stem,
    #[serde(alias = "suffixDiffs")]
    pub suffix_diffs: VariableList<SuffixStateDiff, typenum::U256>,
}

impl TreeHash for StemStateDiff {
    fn tree_hash_type() -> TreeHashType {
        TreeHashType::Container
    }

    fn tree_hash_packed_encoding(&self) -> PackedEncoding {
        unreachable!("StemStateDiff should never be packed.")
    }

    fn tree_hash_packing_factor() -> usize {
        unreachable!("StemStateDiff should never be packed.")
    }

    fn tree_hash_root(&self) -> Hash256 {
        FixedVector::<B256, typenum::U2>::new(vec![
            B256::left_padding_from(self.stem.as_slice()),
            self.suffix_diffs.tree_hash_root(),
        ])
        .expect("stem state diff to fixed B256 vector should succeed")
        .tree_hash_root()
    }
}
