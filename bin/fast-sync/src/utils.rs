use std::{
    ffi::OsStr,
    fs::File,
    io::{BufReader, ErrorKind, Read},
};

use alloy::primitives::B256;
use anyhow::bail;
use discv5::Enr;
use portalnet::discovery::ENR_PORTAL_CLIENT_KEY;
use tracing::info;

pub fn get_client_info(peer: &Enr) -> String {
    match peer
        .get_decodable::<String>(ENR_PORTAL_CLIENT_KEY)
        .and_then(|client| client.ok())
    {
        Some(client) => client,
        None => "?".to_string(),
    }
}

pub fn load_block_hashes(path: &OsStr) -> anyhow::Result<Vec<B256>> {
    info!("load_block_hashes: start");
    let mut reader = BufReader::new(File::open(path)?);
    let mut block_hashes = Vec::with_capacity(15537395);
    let mut buf = [0u8; 32];
    loop {
        if let Err(err) = reader.read_exact(&mut buf) {
            if err.kind() == ErrorKind::UnexpectedEof {
                break;
            }
            bail!("Error reading headers: {err}")
        }
        block_hashes.push(B256::from_slice(&buf));
    }
    info!("load_block_hashes: loaded: {}", block_hashes.len());
    Ok(block_hashes)
}
