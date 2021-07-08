use crate::StorageConfig;
use crypto::hash::Hash;
use models::{Block, BlockHeader, BlockHeaderContent, SerializationContext, Slot};

pub fn get_test_block() -> Block {
    Block {
            header: BlockHeader {
                content: BlockHeaderContent{
                    creator: crypto::signature::PublicKey::from_bs58_check("4vYrPNzUM8PKg2rYPW3ZnXPzy67j9fn5WsGCbnwAnk2Lf7jNHb").unwrap(),
                    operation_merkle_root: get_test_hash(),
                    out_ledger_hash: get_test_hash(),
                    parents: vec![
                        Hash::hash("parent1".as_bytes()),
                        Hash::hash("parent2".as_bytes())
                    ],
                    slot: Slot::new(1, 0),
                },
                signature: crypto::signature::Signature::from_bs58_check(
                    "5f4E3opXPWc3A1gvRVV7DJufvabDfaLkT1GMterpJXqRZ5B7bxPe5LoNzGDQp9LkphQuChBN1R5yEvVJqanbjx7mgLEae"
                ).unwrap()
            },
            operations: vec![]
        }
}

pub fn get_test_config() -> (StorageConfig, SerializationContext) {
    let tempdir = tempfile::tempdir().expect("cannot create temp dir");
    (
        StorageConfig {
            max_stored_blocks: 100000,
            path: tempdir.path().to_path_buf(),
            cache_capacity: 1000000,
            flush_interval: Some(200.into()),
            reset_at_startup: true,
        },
        SerializationContext {
            max_block_size: 1024 * 1024,
            max_block_operations: 1024,
            parent_count: 2,
            max_peer_list_length: 128,
            max_message_size: 3 * 1024 * 1024,
            max_bootstrap_blocks: 100,
            max_bootstrap_cliques: 100,
            max_bootstrap_deps: 100,
            max_bootstrap_children: 100,
        },
    )
}

pub fn get_test_hash() -> Hash {
    Hash::hash("test".as_bytes())
}

pub fn get_another_test_hash() -> Hash {
    Hash::hash("another test".as_bytes())
}
