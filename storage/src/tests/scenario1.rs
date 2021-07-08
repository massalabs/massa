use super::super::{config::StorageConfig, storage_controller::start_storage_controller};


#[tokio::test]
async fn test_get_slot_range() {
	let config = StorageConfig {
    /// Max number of bytes we want to store
   max_capacity: 0, //not used?
    /// path to db
    path: "target/tests/scenario1.db".to_string(),
    pub cache_capacity: 10, //little to force flush cache
    pub flush_every_ms: None, //defaut

	}
}