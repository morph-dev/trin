use ethportal_api::{
    types::{distance::Distance, portal_wire::ProtocolId, verkle::PaginateLocalContentInfo},
    OverlayContentKey,
};
use trin_storage::{
    error::ContentStoreError,
    versioned::{create_store, ContentType, IdIndexedV1Store, IdIndexedV1StoreConfig},
    ContentId, ContentStore, PortalStorageConfig, ShouldWeStoreContent,
};

/// Storage layer for the verkle network. Encapsulates verkle network specific data and logic.
#[derive(Debug)]
pub struct VerkleStorage {
    store: IdIndexedV1Store,
}

impl ContentStore for VerkleStorage {
    fn get<K: OverlayContentKey>(&self, key: &K) -> Result<Option<Vec<u8>>, ContentStoreError> {
        self.store.lookup_content_value(&key.content_id().into())
    }

    fn put<K: OverlayContentKey, V: AsRef<[u8]>>(
        &mut self,
        _key: K,
        _value: V,
    ) -> Result<(), ContentStoreError> {
        // TODO: add verkle specific implementation
        todo!()
    }

    fn is_key_within_radius_and_unavailable<K: OverlayContentKey>(
        &self,
        key: &K,
    ) -> Result<ShouldWeStoreContent, ContentStoreError> {
        let content_id = ContentId::from(key.content_id());
        if self.store.distance_to_content_id(&content_id) > self.store.radius() {
            Ok(ShouldWeStoreContent::NotWithinRadius)
        } else if self.store.has_content(&content_id)? {
            Ok(ShouldWeStoreContent::AlreadyStored)
        } else {
            Ok(ShouldWeStoreContent::Store)
        }
    }

    fn radius(&self) -> Distance {
        IdIndexedV1Store::radius(&self.store)
    }
}

impl VerkleStorage {
    pub fn new(config: PortalStorageConfig) -> Result<Self, ContentStoreError> {
        let sql_connection_pool = config.sql_connection_pool.clone();
        let config = IdIndexedV1StoreConfig::new(ContentType::Verkle, ProtocolId::Verkle, config);
        Ok(Self {
            store: create_store(ContentType::Verkle, config, sql_connection_pool)?,
        })
    }

    /// Returns a paginated list of all locally available content keys, according to the provided
    /// offset and limit.
    pub fn paginate(
        &self,
        offset: u64,
        limit: u64,
    ) -> Result<PaginateLocalContentInfo, ContentStoreError> {
        let paginate_result = self.store.paginate(offset, limit)?;
        Ok(PaginateLocalContentInfo {
            content_keys: paginate_result.content_keys,
            total_entries: paginate_result.entry_count,
        })
    }

    /// Get a summary of the current state of storage
    pub fn get_summary_info(&self) -> String {
        self.store.get_summary_info()
    }
}
