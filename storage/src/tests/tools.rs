use crate::config::StorageConfig;
use crypto::hash::Hash;
use models::{
    block::{Block, BlockHeader},
    slot::Slot,
};

pub fn get_test_block() -> Block {
    Block {
            header: BlockHeader {
                creator: crypto::signature::PublicKey::from_bs58_check("4vYrPNzUM8PKg2rYPW3ZnXPzy67j9fn5WsGCbnwAnk2Lf7jNHb").unwrap(),
                endorsements: vec![],
                operation_merkle_root: get_test_hash(),
                out_ledger_hash: get_test_hash(),
                parents: vec![],
                slot: Slot::new(1, 0),
                roll_number: 0,
            },
            operations: vec![],
            signature: crypto::signature::Signature::from_bs58_check(
                "5f4E3opXPWc3A1gvRVV7DJufvabDfaLkT1GMterpJXqRZ5B7bxPe5LoNzGDQp9LkphQuChBN1R5yEvVJqanbjx7mgLEae"
            ).unwrap()
        }
}

pub fn get_test_config() -> StorageConfig {
    let tempdir = tempfile::tempdir().expect("cannot create temp dir");
    StorageConfig {
        max_stored_blocks: 100000,
        path: tempdir.path().to_path_buf(),
        cache_capacity: 1000000,
        flush_interval: Some(200.into()),
    }
}

pub fn get_test_hash() -> Hash {
    Hash::hash("test".as_bytes())
}

pub fn get_another_test_hash() -> Hash {
    Hash::hash("another test".as_bytes())
}
