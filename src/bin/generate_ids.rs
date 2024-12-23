use std::{fs, os::unix::fs::PermissionsExt, path::PathBuf};

use alloy::primitives::Bytes;
use anyhow::Result;
use discv5::{
    enr::{CombinedKey, NodeId},
    Enr,
};

fn random_node_id() -> (NodeId, CombinedKey) {
    let random_private_key = CombinedKey::generate_secp256k1();
    let enr = Enr::empty(&random_private_key).expect("to be able to generate a random node id");
    (enr.node_id(), random_private_key)
}

pub fn main() -> Result<()> {
    let path = PathBuf::from("/home/milos/Desktop/keys");
    if !path.exists() {
        std::fs::create_dir(&path)?;
    }

    for i in 0..16 {
        let (node_id, key) = loop {
            let (node_id, key) = random_node_id();
            if node_id.as_ref()[0] >> 3 == i << 1 {
                break (node_id, key);
            }
        };
        let key = Bytes::from(key.encode());
        let short_node_id = u32::from_be_bytes(node_id.as_ref()[..4].try_into()?);

        let dir = path.join(&format!("{i:x}"));
        if !dir.exists() {
            std::fs::create_dir(&dir)?;
        }

        println!("{node_id:?} {key}");

        // let file = File::create(dir.join("unsafe_private_key.hex"))?;
        fs::write(dir.join("unsafe_private_key.hex"), format!("{key}"))?;

        let script_path = dir.join("script.sh");
        let commands = format!(
            r#"#!/bin/bash

NODE_ID="{node_id:?}"
SHORT_NODE_ID="{short_node_id:08x}"

echo "NODE_ID: $NODE_ID"

sqlite3 trin.sqlite "PRAGMA cache_size=1000000; UPDATE ii1_state SET distance_short = ( (distance_short | {short_node_id}) & ~(distance_short & {short_node_id}) ); CREATE INDEX ii1_state_distance_short_idx ON ii1_state (distance_short);"

mkdir trin_$SHORT_NODE_ID
mv trin.sqlite trin_$SHORT_NODE_ID
"#
        );
        fs::write(&script_path, commands)?;
        fs::set_permissions(&script_path, fs::Permissions::from_mode(0o770))?;
    }

    Ok(())
}
