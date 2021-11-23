// Copyright (c) 2021 MASSA LABS <info@massa.net>

use super::mock_protocol_controller::MockProtocolController;
use crate::{pool_controller, PoolCommandSender, PoolManager, PoolSettings};
use crypto::hash::Hash;
use futures::Future;
use models::{
    Address, Amount, BlockId, Endorsement, EndorsementContent, Operation, OperationContent,
    OperationType, SerializeCompact, Slot,
};
use signature::{derive_public_key, generate_random_private_key, sign, PrivateKey, PublicKey};
use std::str::FromStr;

pub async fn pool_test<F, V>(
    pool_settings: &'static PoolSettings,
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
        pool_settings,
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

pub fn get_transaction(expire_period: u64, fee: u64) -> (Operation, u8) {
    let sender_priv = generate_random_private_key();
    let sender_pub = derive_public_key(&sender_priv);

    let recv_priv = generate_random_private_key();
    let recv_pub = derive_public_key(&recv_priv);

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
    let signature = sign(&hash, &sender_priv).unwrap();

    (
        Operation { content, signature },
        Address::from_public_key(&sender_pub).unwrap().get_thread(2),
    )
}

/// Creates an endorsement for use in pool tests.
pub fn create_endorsement(slot: Slot) -> Endorsement {
    let sender_priv = generate_random_private_key();
    let sender_public_key = derive_public_key(&sender_priv);

    let content = EndorsementContent {
        sender_public_key,
        slot,
        index: 0,
        endorsed_block: BlockId(Hash::hash("blabla".as_bytes())),
    };
    let hash = Hash::hash(&content.to_bytes_compact().unwrap());
    let signature = sign(&hash, &sender_priv).unwrap();
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
    let signature = sign(&hash, &sender_priv).unwrap();

    (
        Operation { content, signature },
        Address::from_public_key(&sender_pub).unwrap().get_thread(2),
    )
}
