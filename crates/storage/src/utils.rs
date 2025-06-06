use std::{fs, path::Path};

use r2d2::Pool;
use r2d2_sqlite::SqliteConnectionManager;
use tracing::info;

use crate::{
    error::ContentStoreError,
    sql::{
        ENABLE_WAL_MODE, HISTORICAL_SUMMARIES_CREATE_TABLE, LC_BOOTSTRAP_CREATE_TABLE,
        LC_UPDATE_CREATE_TABLE,
    },
    versioned::sql::STORE_INFO_CREATE_TABLE,
    DATABASE_NAME,
};

/// Helper function for opening a SQLite connection.
pub fn setup_sql(node_data_dir: &Path) -> Result<Pool<SqliteConnectionManager>, ContentStoreError> {
    let sql_path = node_data_dir.join(DATABASE_NAME);
    info!(path = %sql_path.display(), "Setting up SqliteDB");

    let manager = SqliteConnectionManager::file(sql_path);
    let pool = Pool::new(manager)?;
    let conn = pool.get()?;
    conn.execute_batch(ENABLE_WAL_MODE)?;
    conn.execute_batch(LC_BOOTSTRAP_CREATE_TABLE)?;
    conn.execute_batch(LC_UPDATE_CREATE_TABLE)?;
    conn.execute_batch(HISTORICAL_SUMMARIES_CREATE_TABLE)?;
    conn.execute_batch(STORE_INFO_CREATE_TABLE)?;
    Ok(pool)
}

/// Internal method used to measure on-disk storage usage.
pub fn get_total_size_of_directory_in_bytes(
    path: impl AsRef<Path>,
) -> Result<u64, ContentStoreError> {
    let metadata = match fs::metadata(&path) {
        Ok(metadata) => metadata,
        Err(_) => {
            return Ok(0);
        }
    };
    let mut size = metadata.len();

    if metadata.is_dir() {
        for entry in fs::read_dir(&path)? {
            let dir = entry?;
            let path_string = match dir.path().into_os_string().into_string() {
                Ok(path_string) => path_string,
                Err(err) => {
                    let err = format!(
                        "Unable to convert path {:?} into string {:?}",
                        path.as_ref(),
                        err
                    );
                    return Err(ContentStoreError::Database(err));
                }
            };
            size += get_total_size_of_directory_in_bytes(path_string)?;
        }
    }

    Ok(size)
}
