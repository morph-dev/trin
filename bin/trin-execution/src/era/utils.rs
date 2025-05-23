use std::time::Duration;

use alloy::primitives::bytes::Bytes;
use e2store::{
    era::Era,
    era1::{BlockTuple, Era1, BLOCK_TUPLE_COUNT},
};
use rayon::iter::{IntoParallelRefIterator, ParallelIterator};
use reqwest::Client;
use tokio::time::sleep;
use tracing::{info, warn};

use super::types::ProcessedEra;
use crate::era::{
    beacon::ProcessBeaconBlock,
    types::{EraType, ProcessedBlock, TransactionsWithSender},
};

pub fn process_era1_file(raw_era1: &[u8]) -> anyhow::Result<ProcessedEra> {
    let mut blocks = Vec::with_capacity(BLOCK_TUPLE_COUNT);
    for BlockTuple { header, body, .. } in Era1::iter_tuples(raw_era1)? {
        let transactions_with_recovered_senders = body
            .body
            .transactions
            .par_iter()
            .map(|tx| {
                tx.recover_signer()
                    .map(|sender_address| TransactionsWithSender {
                        transaction: tx.clone(),
                        sender_address,
                    })
            })
            .collect::<Result<Vec<_>, _>>()?;
        blocks.push(ProcessedBlock {
            header: header.header,
            uncles: Some(body.body.0.ommers),
            withdrawals: None,
            transactions: transactions_with_recovered_senders,
        });
    }

    let epoch_index = Era1::epoch_number_from_block_number(blocks[0].header.number);
    info!("Done processing era1 file {epoch_index}");

    Ok(ProcessedEra {
        first_block_number: blocks[0].header.number,
        blocks,
        era_type: EraType::Era1,
        epoch_index,
    })
}

pub fn process_era_file(era: Era) -> anyhow::Result<ProcessedEra> {
    let epoch_index = era.epoch_index();
    let blocks = era
        .blocks
        .into_iter()
        // Before the merge occurred the beacon chain was already executing bellatrix blocks,
        // because the merge didn't occur yet the execution_payloads were empty. Hence we skip those
        // blocks.
        .filter(|block| block.block.execution_block_number() != 0)
        .map(|block| block.block.process_beacon_block())
        .collect::<anyhow::Result<Vec<_>>>()?;

    info!("Done processing era file {epoch_index}");

    Ok(ProcessedEra {
        first_block_number: blocks[0].header.number,
        blocks,
        era_type: EraType::Era,
        epoch_index,
    })
}

pub async fn download_raw_era(era_path: String, http_client: Client) -> anyhow::Result<Bytes> {
    info!("Downloading era file {era_path}");
    let mut attempts = 1u32;
    let raw_era1 = loop {
        match http_client.get(&era_path).send().await?.bytes().await {
            Ok(raw_era1) => break raw_era1,
            Err(err) => {
                warn!("Failed to download era1 file {attempts} times | Error: {err} | Path {era_path}");
                attempts += 1;
                if attempts > 10 {
                    anyhow::bail!("Failed to download era1 file after {attempts} attempts");
                }
                sleep(Duration::from_secs_f64((attempts as f64).log2())).await;
            }
        }
    };

    info!("Done downloading era file {era_path}");
    Ok(raw_era1)
}
