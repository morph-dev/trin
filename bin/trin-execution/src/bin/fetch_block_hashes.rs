use std::path::PathBuf;

use clap::Parser;
use ethportal_api::{BlockBody, BlockBodyLegacy};
use r2d2::Pool;
use r2d2_sqlite::SqliteConnectionManager;
use revm_primitives::B256;
use ssz::Encode;
use tracing::info;
use trin_execution::era::manager::EraManager;
use trin_utils::log::init_tracing_logger;

#[derive(Parser, Debug, Clone)]
#[command(name = "Block header fetcher")]
struct Args {
    #[arg(help = "The last block to fetch")]
    last_block: u64,

    #[arg(help = "The Path to the sqlite db")]
    db_path: PathBuf,
}

mod sql {
    pub const DB_INIT: &str = "
        PRAGMA journal_mode=WAL;
        PRAGMA synchronous = NORMAL;
    ";

    pub const CREATE_TABLE: &str = "
        CREATE TABLE IF NOT EXISTS headers (
            number INTEGER PRIMARY KEY,
            hash BLOB NOT NULL,
            header BLOB NOT NULL,
            body BLOB NOT NULL
        );
        CREATE INDEX IF NOT EXISTS headers_hash_idx ON headers (hash);
    ";

    pub const QUERY_LAST_BLOCK: &str =
        "SELECT number, hash FROM headers ORDER BY number DESC LIMIT 1;";

    pub const INSERT_BLOCK: &str =
        "INSERT INTO headers (number, hash, header, body) VALUES (?1, ?2, ?3, ?4);";
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    init_tracing_logger();

    let args = Args::parse();

    let sqlite_pool = Pool::new(SqliteConnectionManager::file(args.db_path).with_init(|c| {
        c.execute_batch(sql::DB_INIT)?;
        c.execute_batch(sql::CREATE_TABLE)?;
        Ok(())
    }))?;

    let conn = sqlite_pool.get()?;

    let last_block: Option<(u64, B256)> = conn
        .prepare(sql::QUERY_LAST_BLOCK)?
        .query_map((), |row| {
            let number = row.get::<_, u64>("number")?;
            let hash = B256::from_slice(&row.get::<_, Vec<u8>>("hash")?);
            Ok((number, hash))
        })?
        .next()
        .transpose()?;

    let next_block_number = match last_block {
        Some((number, _hash)) => number + 1,
        None => 0,
    };
    let mut last_block_hash = last_block.map(|(_number, hash)| hash);

    info!("Next block number: {next_block_number}");
    if next_block_number >= args.last_block {
        info!("Nothing to do!");
        return Ok(());
    }

    let mut total_headers_size = 0;
    let mut total_bodies_size = 0;

    let mut insert_query = conn.prepare(sql::INSERT_BLOCK)?;

    let mut era_manager = EraManager::new(next_block_number).await?;
    while era_manager.next_block_number() <= args.last_block {
        let block = era_manager.get_next_block().await?;
        let header = block.header.clone();
        if header.number % 100_000 == 0 {
            info!(
                total_headers_size,
                total_bodies_size, "Writing block: {}", header.number,
            );
        }

        let hash = header.hash();

        if let Some(last_block_hash) = last_block_hash {
            anyhow::ensure!(
                last_block_hash == header.parent_hash,
                "Invalid parent block. Expected: {last_block_hash}, actual header: {header:?}"
            );
        }
        last_block_hash = Some(hash);

        let block_body = BlockBody::Legacy(BlockBodyLegacy {
            txs: block
                .transactions
                .iter()
                .map(|tx| tx.transaction.clone())
                .collect(),
            uncles: block.uncles.to_owned().unwrap_or_default(),
        });

        let header_bytes = alloy_rlp::encode(&header);
        total_headers_size += header_bytes.len();
        let block_body_bytes = block_body.as_ssz_bytes();
        total_bodies_size += block_body_bytes.len();
        let inserted = insert_query.execute((
            header.number,
            header.hash().to_vec(),
            header_bytes,
            block_body_bytes,
        ))?;

        anyhow::ensure!(inserted == 1, "Expected to insert one header");
    }

    info!(total_headers_size, total_bodies_size, "Finished!");

    Ok(())
}
