use std::{
    fs::File,
    io::{BufWriter, Write},
    path::PathBuf,
};

use anyhow::ensure;
use clap::Parser;
use r2d2::Pool;
use r2d2_sqlite::SqliteConnectionManager;
use revm_primitives::B256;
use tracing::info;
use trin_utils::log::init_tracing_logger;

#[derive(Parser, Debug, Clone)]
#[command(name = "Extracts block hashes from sqlite db into binary files")]
struct Args {
    #[arg(help = "The Path to the sqlite db")]
    db_path: PathBuf,
    #[arg(help = "The Path to the binary file")]
    path: PathBuf,
}

mod sql {
    pub const QUERY: &str = "SELECT number, hash FROM headers ORDER BY number;";
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    init_tracing_logger();

    let args = Args::parse();

    let sqlite_pool = Pool::new(SqliteConnectionManager::file(args.db_path))?;
    let conn = sqlite_pool.get()?;

    let mut binary_file = BufWriter::new(File::create(args.path)?);

    info!("Reading db");

    let mut query = conn.prepare(sql::QUERY)?;
    let rows = query.query_map((), |row| {
        let number = row.get::<_, u64>("number")?;
        let hash = B256::from_slice(&row.get::<_, Vec<u8>>("hash")?);
        Ok((number, hash))
    })?;

    info!("Starting to write");

    let mut count = 0;

    for row in rows {
        if count % 100_000 == 0 {
            info!("{count} headers written")
        }

        let (number, hash) = row.expect("Reading row should be successful!");

        ensure!(count == number, "Expected {count} but got {number}");
        ensure!(binary_file.write(hash.as_slice())? == B256::len_bytes());

        count += 1;
    }

    binary_file.flush()?;

    info!("Finished! Total hashes written: {count}");

    Ok(())
}
