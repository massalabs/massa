// Copyright (c) 2021 MASSA LABS <info@massa.net>

use super::mock_protocol_controller::MockProtocolController;
use crate::{pool_controller, PoolCommandSender, PoolConfig, PoolManager};
use crypto::{
    hash::Hash,
    signature::{PrivateKey, PublicKey},
};
use futures::Future;
use models::{
    Address, Amount, BlockId, Endorsement, EndorsementContent, Operation, OperationContent,
    OperationType, SerializeCompact, Slot,
};
use std::str::FromStr;

pub async fn pool_test<F, V>(
    cfg: PoolConfig,
    thread_count: u8,
    operation_validity_periods: u64,
    test: F,
) where
    F: FnOnce(MockProtocolController, PoolCommandSender, PoolManager) -> V,
    V: Future<Output = (MockProtocolController, PoolCommandSender, PoolManager)>,
{
    let (protocol_controller, protocol_command_sender, protocol_pool_event_receiver) =
        MockProtocolController::new();

    let (pool_command_sender, pool_manager) = pool_controller::start_pool_controller(
        cfg.clone(),
        thread_count,
        operation_validity_periods,
        protocol_command_sender,
        protocol_pool_event_receiver,
    )
    .await
    .unwrap();

    let (_protocol_controller, _pool_command_sender, pool_manager) =
        test(protocol_controller, pool_command_sender, pool_manager).await;

    pool_manager.stop().await.unwrap();
}

pub fn example_pool_config() -> (PoolConfig, u8, u64) {
    let mut nodes = Vec::new();
    for _ in 0..2 {
        let private_key = crypto::generate_random_private_key();
        let public_key = crypto::derive_public_key(&private_key);
        nodes.push((public_key, private_key));
    }
    let thread_count: u8 = 2;
    let operation_validity_periods: u64 = 50;
    let max_block_size = 1024 * 1024;
    let max_operations_per_block = 1024;

    // Init the serialization context with a default,
    // can be overwritten with a more specific one in the test.
    models::init_serialization_context(models::SerializationContext {
        max_block_operations: max_operations_per_block,
        parent_count: thread_count,
        max_peer_list_length: 128,
        max_message_size: 3 * 1024 * 1024,
        max_block_size: max_block_size,
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
    });

    (
        PoolConfig {
            max_pool_size_per_thread: 100000,
            max_operation_future_validity_start_periods: 200,
        },
        thread_count,
        operation_validity_periods,
    )
}

pub fn get_transaction(expire_period: u64, fee: u64) -> (Operation, u8) {
    let sender_priv = crypto::generate_random_private_key();
    let sender_pub = crypto::derive_public_key(&sender_priv);

    let recv_priv = crypto::generate_random_private_key();
    let recv_pub = crypto::derive_public_key(&recv_priv);

    let op = OperationType::Transaction {
        recipient_address: Address::from_public_key(&recv_pub).unwrap(),
        amount: Amount::default(),
    };
    let content = OperationContent {
        fee: Amount::from_str(&fee.to_string()).unwrap(),
        op,
        sender_public_key: sender_pub,
        expire_period,
    };
    let hash = Hash::hash(&content.to_bytes_compact().unwrap());
    let signature = crypto::sign(&hash, &sender_priv).unwrap();

    (
        Operation { content, signature },
        Address::from_public_key(&sender_pub).unwrap().get_thread(2),
    )
}

/// Creates an endorsement for use in pool tests.
pub fn create_endorsement(slot: Slot) -> Endorsement {
    let sender_priv = crypto::generate_random_private_key();
    let sender_public_key = crypto::derive_public_key(&sender_priv);

    let content = EndorsementContent {
        sender_public_key,
        slot,
        index: 0,
        endorsed_block: BlockId(Hash::hash("blabla".as_bytes())),
    };
    let hash = Hash::hash(&content.to_bytes_compact().unwrap());
    let signature = crypto::sign(&hash, &sender_priv).unwrap();
    Endorsement {
        content: content.clone(),
        signature,
    }
}

pub fn get_transaction_with_addresses(
    expire_period: u64,
    fee: u64,
    sender_pub: PublicKey,
    sender_priv: PrivateKey,
    recv_pub: PublicKey,
) -> (Operation, u8) {
    let op = OperationType::Transaction {
        recipient_address: Address::from_public_key(&recv_pub).unwrap(),
        amount: Amount::default(),
    };
    let content = OperationContent {
        fee: Amount::from_str(&fee.to_string()).unwrap(),
        op,
        sender_public_key: sender_pub,
        expire_period,
    };
    let hash = Hash::hash(&content.to_bytes_compact().unwrap());
    let signature = crypto::sign(&hash, &sender_priv).unwrap();

    (
        Operation { content, signature },
        Address::from_public_key(&sender_pub).unwrap().get_thread(2),
    )
}
