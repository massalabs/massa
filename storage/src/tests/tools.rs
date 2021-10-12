// Copyright (c) 2021 MASSA LABS <info@massa.net>

use crate::{start_storage, StorageAccess, StorageConfig};
use crypto::hash::Hash;
use models::hhasher::BuildHHasher;
use models::{
    Address, Amount, Block, BlockHeader, BlockHeaderContent, BlockId, Operation, OperationContent,
    OperationId, OperationType, SerializationContext, Slot,
};
use models::{OperationHashMap, SerializeCompact};
use std::future::Future;

/// Runs a storage test, passing the storage access to it.
pub async fn storage_test<F, V>(cfg: StorageConfig, test: F)
where
    F: FnOnce(StorageAccess) -> V,
    V: Future<Output = ()>,
{
    let (storage, manager) = start_storage(cfg).unwrap();

    // Call test func.
    test(storage).await;

    manager.stop().await.unwrap();
}

pub fn get_dummy_block_id(s: &str) -> BlockId {
    BlockId(Hash::hash(s.as_bytes()))
}

pub fn get_test_block() -> Block {
    Block {
            header: BlockHeader {
                content: BlockHeaderContent{
                    creator: crypto::signature::PublicKey::from_bs58_check("4vYrPNzUM8PKg2rYPW3ZnXPzy67j9fn5WsGCbnwAnk2Lf7jNHb").unwrap(),
                    operation_merkle_root: Hash::hash(&Vec::new()),
                    parents: vec![
                        get_dummy_block_id("parent1"),
						get_dummy_block_id("parent2"),
                    ],
                    slot: Slot::new(1, 0),
                    endorsements: Vec::new(),
                },
                signature: crypto::signature::Signature::from_bs58_check(
                    "5f4E3opXPWc3A1gvRVV7DJufvabDfaLkT1GMterpJXqRZ5B7bxPe5LoNzGDQp9LkphQuChBN1R5yEvVJqanbjx7mgLEae"
                ).unwrap()
            },
            operations: vec![]
        }
}

pub fn create_operation() -> Operation {
    let sender_priv = crypto::generate_random_private_key();
    let sender_pub = crypto::derive_public_key(&sender_priv);

    let recv_priv = crypto::generate_random_private_key();
    let recv_pub = crypto::derive_public_key(&recv_priv);

    let op = OperationType::Transaction {
        recipient_address: Address::from_public_key(&recv_pub).unwrap(),
        amount: Amount::default(),
    };
    let content = OperationContent {
        fee: Amount::default(),
        op,
        sender_public_key: sender_pub,
        expire_period: 0,
    };
    let hash = Hash::hash(&content.to_bytes_compact().unwrap());
    let signature = crypto::sign(&hash, &sender_priv).unwrap();

    Operation { content, signature }
}

pub fn get_block_with_op() -> (Block, BlockId, OperationId) {
    let op = create_operation();
    let block = Block {
        header: BlockHeader {
            content: BlockHeaderContent{
                creator: crypto::signature::PublicKey::from_bs58_check("4vYrPNzUM8PKg2rYPW3ZnXPzy67j9fn5WsGCbnwAnk2Lf7jNHb").unwrap(),
                operation_merkle_root: Hash::hash(&Vec::new()),
                parents: vec![
                    get_dummy_block_id("parent1"),
                    get_dummy_block_id("parent2"),
                ],
                slot: Slot::new(1, 0),
                endorsements: Vec::new(),
            },
            signature: crypto::signature::Signature::from_bs58_check(
                "5f4E3opXPWc3A1gvRVV7DJufvabDfaLkT1GMterpJXqRZ5B7bxPe5LoNzGDQp9LkphQuChBN1R5yEvVJqanbjx7mgLEae"
            ).unwrap()
        },
        operations: vec![op.clone()]
    };
    let id = block.header.compute_block_id().unwrap();
    (block, id, op.get_operation_id().unwrap())
}

pub fn get_test_config() -> StorageConfig {
    let tempdir = tempfile::tempdir().expect("cannot create temp dir");
    let context = SerializationContext {
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
        max_operations_per_message: 1024,
        max_endorsements_per_message: 1024,
        max_bootstrap_message_size: 100000000,
        max_bootstrap_pos_entries: 1000,
        max_bootstrap_pos_cycles: 5,
        max_block_endorsments: 8,
    };
    models::init_serialization_context(context.clone());
    StorageConfig {
        max_stored_blocks: 100000,
        path: tempdir.path().to_path_buf(),
        cache_capacity: 1000000,
        flush_interval: Some(200.into()),
        reset_at_startup: true,
    }
}

pub fn get_test_block_id() -> BlockId {
    get_dummy_block_id("test")
}

pub fn get_another_test_block_id() -> BlockId {
    get_dummy_block_id("another test")
}

pub fn get_operation_set(operations: &Vec<Operation>) -> OperationHashMap<(usize, u64)> {
    let mut res =
        OperationHashMap::with_capacity_and_hasher(operations.len(), BuildHHasher::default());
    for (idx, op) in operations.iter().enumerate() {
        let op_id = op.get_operation_id().unwrap();
        res.insert(op_id, (idx, op.content.expire_period));
    }
    res
}
