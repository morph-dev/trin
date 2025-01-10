use std::{fs, path::PathBuf, time::Instant};

use alloy::{
    hex::FromHex,
    primitives::{B256, U256},
};
use anyhow::{ensure, Result};
use clap::Parser;
use discv5::{enr::CombinedKey, Enr};
use r2d2::ManageConnection;
use r2d2_sqlite::SqliteConnectionManager;
use tracing::{debug, info};
use trin_utils::log::init_tracing_logger;

#[derive(Parser, Debug, PartialEq)]
#[command(name = "")]
pub struct Config {
    db_path: PathBuf,
    #[arg(long)]
    private_key_path: Option<PathBuf>,
    #[arg(long, default_value_t = 2)]
    split: usize,
}

const FLAG: u64 = 1 << 32;

pub fn main() -> Result<()> {
    init_tracing_logger();

    let config = Config::parse();

    ensure!(
        config.db_path.exists(),
        "DB doesn't exist: {}",
        config.db_path.to_string_lossy()
    );

    let sqlite_manager = SqliteConnectionManager::file(&config.db_path);
    let conn = sqlite_manager.connect()?;

    let private_key = match config.private_key_path {
        Some(private_key_path) => {
            ensure!(private_key_path.exists());

            let private_key = fs::read_to_string(private_key_path)?;
            let mut private_key = B256::from_hex(private_key.trim())?;
            Some(CombinedKey::secp256k1_from_bytes(
                private_key.as_mut_slice(),
            )?)
        }
        None => None,
    };

    let start_time = Instant::now();

    let query_base = match private_key {
        Some(private_key) => {
            let node_id = Enr::empty(&private_key)?.node_id();
            let short_node_id = u32::from_be_bytes(node_id.as_ref()[..4].try_into()?);
            let update_value = FLAG ^ short_node_id as u64;
            info!("Updating for node id {node_id} with value {update_value} (0x{update_value:x})");
            format!(
                "UPDATE ii1_state SET distance_short = (distance_short | {}) & ~(distance_short & {}) WHERE distance_short >= {}",
                update_value,
                update_value,
                FLAG,
            )
        }
        None => {
            info!("Updating with value {FLAG} (0x{FLAG:x})");
            format!(
                "UPDATE ii1_state SET distance_short = distance_short | {} WHERE distance_short < {}",
                FLAG,
                FLAG,
            )
        }
    };
    info!("query_base: {query_base}");

    let mut total_updated = 0;

    let mut start = U256::ZERO;
    let step = U256::MAX / U256::from(config.split);

    while start < U256::MAX {
        let end = start.saturating_add(step);

        let step_start_time = Instant::now();

        let query =
            format!("{query_base} AND content_id BETWEEN x'{start:064x}' AND x'{end:064x}'");
        info!("Running for range: {start:064x} and {end:064x}");
        debug!("Query: {query}");
        let updated = conn.execute(&query, [])?;
        info!(
            "Updated: {updated} in {:.2} seconds",
            step_start_time.elapsed().as_secs_f32()
        );
        total_updated += updated;

        start = end.saturating_add(U256::from(1));
    }

    info!("Total updated: {total_updated}");
    info!("Duration: {:.2}", start_time.elapsed().as_secs_f32());
    Ok(())
}
