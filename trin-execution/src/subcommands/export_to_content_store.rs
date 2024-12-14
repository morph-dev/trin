use std::{fs, iter, path::Path, sync::Arc, time::Instant};

use alloy::{
    consensus::{constants::KECCAK_EMPTY, EMPTY_ROOT_HASH},
    primitives::B256,
    rlp::Decodable,
};
use anyhow::anyhow;
use eth_trie::node::Node;
use ethportal_api::{
    types::{
        content_key::state::{AccountTrieNodeKey, ContractBytecodeKey, ContractStorageTrieNodeKey},
        content_value::state::{ContractBytecode, TrieNode},
        state_trie::{account_state::AccountState, nibbles::Nibbles, ByteCode, EncodedTrieNode},
    },
    ContentValue, OverlayContentKey, RawContentKey, RawContentValue, StateContentKey,
    StateContentValue,
};
use humanize_duration::{prelude::DurationExt, Truncate};
use r2d2::ManageConnection;
use r2d2_sqlite::{rusqlite::Connection, SqliteConnectionManager};
use tracing::info;
use trin_storage::versioned::{id_indexed_v1, ContentType};

use crate::{
    cli::ExportToContentStoreConfig,
    storage::{
        account_db::AccountDB, execution_position::ExecutionPosition, trie_db::TrieRocksDB,
        utils::setup_rocksdb,
    },
    trie_walker::TrieWalker,
};

pub async fn export(config: ExportToContentStoreConfig, data_dir: &Path) -> anyhow::Result<()> {
    // Initialize RocksDb and execution position
    assert!(
        fs::exists(data_dir.join("rocksdb"))?,
        "RocksDb directory is missing!"
    );
    let rocks_db = Arc::new(setup_rocksdb(data_dir)?);
    let execution_position = ExecutionPosition::initialize_from_db(rocks_db.clone())?;
    let state_root = execution_position.state_root();
    assert!(execution_position.next_block_number() > 0, "Empty RocksDb");
    assert!(state_root != EMPTY_ROOT_HASH, "Empty state trie");
    info!(
        "Converting state trie for block {} with state root {}",
        execution_position.next_block_number() - 1,
        state_root
    );
    let trie_db = TrieRocksDB::new(false, rocks_db.clone());

    // Make sure Content Store directory exists
    let content_store_dir = &config.path_to_content_store;
    if fs::exists(content_store_dir)? {
        assert!(
            fs::metadata(content_store_dir)?.is_dir(),
            "The data_dir \"{content_store_dir:?}\" exists and is not directory",
        );
    } else {
        fs::create_dir_all(content_store_dir)?;
    }
    // Create content store
    let mut content_store = ContentStore::new(&config)?;

    // Iterate Account trie
    let account_start_time = Instant::now();
    for (account_trie_node_index, trie_proof) in
        TrieWalker::new(state_root, Arc::new(trie_db))?.enumerate()
    {
        if account_trie_node_index % 100_000 == 0 {
            print_progress("Account", account_start_time, &trie_proof.path);
        }

        // Create and store AccountTrieNode
        let encoded_trie_node: EncodedTrieNode = trie_proof
            .proof
            .last()
            .expect("empty proof")
            .to_vec()
            .into();
        let trie_node = encoded_trie_node.as_trie_node()?;

        content_store.insert(
            StateContentKey::AccountTrieNode(AccountTrieNodeKey {
                path: Nibbles::try_from_unpacked_nibbles(&trie_proof.path)?,
                node_hash: encoded_trie_node.node_hash(),
            }),
            StateContentValue::TrieNode(TrieNode {
                node: encoded_trie_node,
            }),
        )?;

        // Continue Account Trie traversal if trie node is not leaf node
        let Node::Leaf(leaf_node) = trie_node else {
            continue;
        };

        let account_state = AccountState::decode(&mut leaf_node.value.as_ref())?;

        // Calculate address hash by concatanating node's path, and key of the leaf node
        let mut path_nibbles = eth_trie::nibbles::Nibbles::from_hex(&trie_proof.path);
        path_nibbles.extend(&leaf_node.key);
        let (path, is_leaf) = path_nibbles.encode_raw();
        assert!(is_leaf, "Leaf node doesn't have leaf path");
        assert_eq!(
            path.len(),
            32,
            "Length of the path of the leaf node should be 32"
        );
        let address_hash = B256::from_slice(&path);

        // Store ContractBytecode if code is non-empty
        if account_state.code_hash != KECCAK_EMPTY {
            let bytecode = rocks_db.get(account_state.code_hash)?.ok_or(anyhow!(
                "Bytecode with hash {} not found in db.",
                account_state.code_hash
            ))?;
            content_store.insert(
                StateContentKey::ContractBytecode(ContractBytecodeKey {
                    address_hash,
                    code_hash: account_state.code_hash,
                }),
                StateContentValue::ContractBytecode(ContractBytecode {
                    code: ByteCode::new(bytecode).expect("To create bytecode from code"),
                }),
            )?;
        }

        // Iterate Storage trie if not empty
        if account_state.storage_root != EMPTY_ROOT_HASH {
            let account_db = AccountDB::new(address_hash, rocks_db.clone());

            let storage_start_time = Instant::now();
            for (storage_trie_node_index, storage_trie_proof) in
                TrieWalker::new(account_state.storage_root, Arc::new(account_db))?.enumerate()
            {
                if storage_trie_node_index > 0 && storage_trie_node_index % 100_000 == 0 {
                    print_progress(
                        &format!("Storage ({address_hash})"),
                        storage_start_time,
                        &storage_trie_proof.path,
                    );
                }

                let trie_node: EncodedTrieNode = storage_trie_proof
                    .proof
                    .last()
                    .expect("empty proof")
                    .to_vec()
                    .into();

                content_store.insert(
                    StateContentKey::ContractStorageTrieNode(ContractStorageTrieNodeKey {
                        address_hash,
                        path: Nibbles::try_from_unpacked_nibbles(&storage_trie_proof.path)?,
                        node_hash: trie_node.node_hash(),
                    }),
                    StateContentValue::TrieNode(TrieNode { node: trie_node }),
                )?;
            }
        }
    }

    content_store.flush()?;

    info!(
        "Finished in {}",
        account_start_time.elapsed().human(Truncate::Second)
    );

    Ok(())
}

fn print_progress(prefix: &str, start_time: Instant, nibbles: &[u8]) {
    let mut path_u32 = 0u32;
    for nibble in nibbles.iter().chain(iter::repeat(&0)).take(4) {
        path_u32 = (path_u32 << 4) | *nibble as u32;
    }
    let progress = 100.0 * path_u32 as f64 / 0xFFFFu32 as f64;
    info!(
        "{prefix} progress (0x{path_u32:04x}): {progress:.2}% done in {}",
        start_time.elapsed().human(Truncate::Second)
    );
}

struct ContentStore {
    _sql_manager: SqliteConnectionManager,
    conn: Connection,
    cache: Vec<(B256, RawContentKey, RawContentValue)>,
}

impl ContentStore {
    const CACHE_SIZE: usize = 10_000_000;

    fn new(config: &ExportToContentStoreConfig) -> anyhow::Result<Self> {
        let path = config.path_to_content_store.join("trin.sqlite");

        // Make sure content store doesn't already exist
        if fs::exists(&path)? && config.override_content_store {
            fs::remove_file(&path)?;
        }
        assert!(!fs::exists(&path)?, "Store file {path:?} already exists");

        // Create sqlite manager and connection
        let sql_manager = SqliteConnectionManager::file(path)
            .with_init(|conn| conn.execute_batch("PRAGMA journal_mode=WAL;"));
        let conn = sql_manager.connect()?;
        conn.execute_batch(&id_indexed_v1::sql::create_table(&ContentType::State))?;

        Ok(Self {
            _sql_manager: sql_manager,
            conn,
            cache: Vec::with_capacity(Self::CACHE_SIZE),
        })
    }

    fn insert(&mut self, key: StateContentKey, value: StateContentValue) -> anyhow::Result<()> {
        let id = key.content_id();
        let key = key.to_bytes();
        let value = value.encode();
        self.cache.push((id.into(), key, value));
        if self.cache.len() >= Self::CACHE_SIZE {
            self.flush()?;
        }
        Ok(())
    }

    fn flush(&mut self) -> anyhow::Result<()> {
        if self.cache.is_empty() {
            return Ok(());
        }

        info!("Writting to content store");
        self.cache.sort();
        let tx = self.conn.transaction()?;
        for (id, key, value) in &self.cache {
            let content_size = id.len() + key.len() + value.len();
            let distance_u32 = u32::from_be_bytes(id[..4].try_into()?);
            tx.execute(
                &id_indexed_v1::sql::insert(&ContentType::State),
                (
                    id.as_slice(),
                    key.to_vec(),
                    value.to_vec(),
                    distance_u32,
                    content_size,
                ),
            )?;
        }
        tx.commit()?;
        self.cache.clear();
        Ok(())
    }
}

impl Drop for ContentStore {
    fn drop(&mut self) {
        self.flush().expect("Flushing to be successful");
        self.conn
            .execute_batch("PRAGMA journal_mode=DELETE;")
            .expect("to rollback journal mode");
    }
}
