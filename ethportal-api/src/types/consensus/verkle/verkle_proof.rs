use std::iter;

use alloy_primitives::{Bytes, B256};
use portal_verkle_primitives::{proof::IpaProof, Point, Stem};
use serde::{Deserialize, Serialize};
use ssz_derive::{Decode, Encode};
use ssz_types::{typenum, FixedVector, VariableList};
use tree_hash::{Hash256, PackedEncoding, TreeHash, TreeHashType};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Encode, Decode)]
#[serde(deny_unknown_fields)]
pub struct VerkleProof {
    #[serde(alias = "otherStems")]
    pub other_stems: VariableList<Stem, typenum::U65536>,
    #[serde(alias = "depthExtensionPresent")]
    pub depth_extension_present: Bytes, // MAX LEN = 2**16 * 33
    #[serde(alias = "commitmentsByPath")]
    pub commitments_by_path: VariableList<Point, typenum::U4194304>,
    pub d: Point,
    #[serde(alias = "ipaProof")]
    pub ipa_proof: IpaProof,
}

impl TreeHash for VerkleProof {
    fn tree_hash_type() -> TreeHashType {
        TreeHashType::Container
    }

    fn tree_hash_packed_encoding(&self) -> PackedEncoding {
        unreachable!("VerkleProof should never be packed.")
    }

    fn tree_hash_packing_factor() -> usize {
        unreachable!("VerkleProof should never be packed.")
    }

    fn tree_hash_root(&self) -> Hash256 {
        let other_stems = VariableList::<B256, typenum::U65536>::new(
            self.other_stems
                .iter()
                .map(|stem| B256::left_padding_from(stem.as_slice()))
                .collect(),
        )
        .expect("other_stems to variable B256 list should succeed");

        let depth_extension_present = VariableList::<B256, typenum::U524288>::new(
            self.depth_extension_present
                .chunks(32)
                .map(B256::left_padding_from)
                .collect(),
        )
        .expect("depth_extension_present to variable B256 list should succeed");

        let commitments_by_path = VariableList::<B256, typenum::U4194304>::new(
            self.commitments_by_path.iter().map(B256::from).collect(),
        )
        .expect("commitments_by_path to variable B256 list should succeed");

        let ipa_proof = FixedVector::<B256, typenum::U17>::new(
            iter::empty()
                .chain(&self.ipa_proof.cl)
                .chain(&self.ipa_proof.cr)
                .map(B256::from)
                .chain(iter::once(B256::from(&self.ipa_proof.final_evaluation)))
                .collect(),
        )
        .expect("ipa_proof to fixed B256 vector should succeed");

        FixedVector::<B256, typenum::U5>::new(vec![
            other_stems.tree_hash_root(),
            depth_extension_present.tree_hash_root(),
            commitments_by_path.tree_hash_root(),
            B256::from(&self.d),
            ipa_proof.tree_hash_root(),
        ])
        .expect("verkle proof to fixed B256 vector should succeed")
        .tree_hash_root()
    }
}
