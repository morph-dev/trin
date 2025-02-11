use std::path::PathBuf;

use clap::Parser;
use r2d2::Pool;
use r2d2_sqlite::SqliteConnectionManager;
use revm_primitives::B256;
use tracing::info;
use trin_execution::era::manager::EraManager;
use trin_utils::log::init_tracing_logger;

#[derive(Parser, Debug, Clone)]
#[command(name = "Block header fetcher")]
struct Args {
    #[arg(help = "The last block to fetch")]
    last_block: u64,

    #[arg(help = "The Path to the sqlite db", long)]
    db_path: PathBuf,
}

mod sql {
    pub const DB_INIT: &str = "
        PRAGMA journal_mode=WAL;
        PRAGMA synchronous = NORMAL;
    ";

    pub const CREATE_TABLE: &str = "
        CREATE TABLE IF NOT EXISTS headers (number INTEGER PRIMARY KEY, hash BLOB NOT NULL);
        CREATE INDEX IF NOT EXISTS headers_hash_idx ON headers (hash);
    ";

    pub const QUERY_LAST_BLOCK: &str =
        "SELECT number, hash FROM headers ORDER BY number DESC LIMIT 1;";

    pub const INSERT_BLOCK: &str = "INSERT INTO headers (number, hash) VALUES (?1, ?2);";
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

    let mut insert_query = conn.prepare(sql::INSERT_BLOCK)?;

    let mut era_manager = EraManager::new(next_block_number).await?;
    while era_manager.next_block_number() <= args.last_block {
        let header = &era_manager.get_next_block().await?.header;
        if header.number % 100_000 == 0 {
            info!("Writing block: {}", header.number);
        }

        let hash = header.hash();

        if let Some(last_block_hash) = last_block_hash {
            anyhow::ensure!(
                last_block_hash == header.parent_hash,
                "Invalid parent block. Expected: {last_block_hash}, actual header: {header:?}"
            );
        }
        last_block_hash = Some(hash);

        let inserted = insert_query.execute((
            header.number,
            header.hash().to_vec(),
            // alloy_rlp::encode(header),
        ))?;

        anyhow::ensure!(inserted == 1, "Expected to insert one header");
    }

    info!("Finished!");

    Ok(())
}
