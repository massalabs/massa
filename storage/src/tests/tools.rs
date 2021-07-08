use crate::StorageConfig;
use crypto::hash::Hash;
use models::{Block, BlockHeader, BlockHeaderContent, BlockId, SerializationContext, Slot};

pub fn get_test_block() -> Block {
    Block {
            header: BlockHeader {
                content: BlockHeaderContent{
                    creator: crypto::signature::PublicKey::from_bs58_check("4vYrPNzUM8PKg2rYPW3ZnXPzy67j9fn5WsGCbnwAnk2Lf7jNHb").unwrap(),
                    operation_merkle_root: Hash::hash("test".as_bytes()),
                    out_ledger_hash: Hash::hash("test".as_bytes()),
                    parents: vec![
                        BlockId::for_tests("parent1").unwrap(),
						BlockId::for_tests("parent2").unwrap(),
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
            max_ask_blocks_per_message: 10,
            max_bootstrap_message_size: 100000000,
        },
    )
}

pub fn get_test_block_id() -> BlockId {
    BlockId::for_tests("test").unwrap()
}

pub fn get_another_test_block_id() -> BlockId {
    BlockId::for_tests("another test").unwrap()
}
