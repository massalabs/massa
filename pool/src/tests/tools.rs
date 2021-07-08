use crypto::{hash::Hash, signature::SignatureEngine};
use models::{
    Address, Operation, OperationContent, OperationType, SerializationContext, SerializeCompact,
};

use crate::PoolConfig;

pub fn example_pool_config() -> (PoolConfig, SerializationContext, u8, u64) {
    let sig_engine = SignatureEngine::new();
    let mut nodes = Vec::new();
    for _ in 0..2 {
        let private_key = SignatureEngine::generate_random_private_key();
        let public_key = sig_engine.derive_public_key(&private_key);
        nodes.push((public_key, private_key));
    }
    let thread_count: u8 = 2;
    let operation_validity_periods: u64 = 50;
    let max_block_size = 1024 * 1024;
    let max_operations_per_block = 1024;
    (
        PoolConfig {
            max_pool_size_per_thread: 100000,
            max_operation_future_validity_start_periods: 200,
        },
        SerializationContext {
            max_block_size,
            max_block_operations: max_operations_per_block,
            parent_count: thread_count,
            max_peer_list_length: 128,
            max_message_size: 3 * 1024 * 1024,
            max_bootstrap_blocks: 100,
            max_bootstrap_cliques: 100,
            max_bootstrap_deps: 100,
            max_bootstrap_children: 100,
            max_ask_blocks_per_message: 10,
            max_operations_per_message: 1024,
            max_bootstrap_message_size: 100000000,
        },
        thread_count,
        operation_validity_periods,
    )
}

pub fn get_transaction(
    expire_period: u64,
    fee: u64,
    context: &SerializationContext,
) -> (Operation, u8) {
    let sig_engine = SignatureEngine::new();
    let sender_priv = SignatureEngine::generate_random_private_key();
    let sender_pub = sig_engine.derive_public_key(&sender_priv);

    let recv_priv = SignatureEngine::generate_random_private_key();
    let recv_pub = sig_engine.derive_public_key(&recv_priv);

    let op = OperationType::Transaction {
        recipient_address: Address::from_public_key(&recv_pub).unwrap(),
        amount: 0,
    };
    let content = OperationContent {
        fee,
        op,
        sender_public_key: sender_pub,
        expire_period,
    };
    let hash = Hash::hash(&content.to_bytes_compact(context).unwrap());
    let signature = sig_engine.sign(&hash, &sender_priv).unwrap();

    (
        Operation { content, signature },
        Address::from_public_key(&sender_pub).unwrap().get_thread(2),
    )
}
