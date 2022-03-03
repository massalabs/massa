// Copyright (c) 2022 MASSA LABS <info@massa.net>

use super::mock_protocol_controller::MockProtocolController;
use crate::{pool_controller, settings::PoolConfig, PoolCommandSender, PoolManager};
use futures::Future;
use massa_hash::hash::Hash;
use massa_models::{
    signed::Signed, Address, Amount, BlockId, Endorsement, EndorsementId, Operation, OperationId,
    OperationType, Slot,
};
use massa_signature::{derive_public_key, generate_random_private_key, PrivateKey, PublicKey};
use std::str::FromStr;

pub async fn pool_test<F, V>(cfg: &'static PoolConfig, test: F)
where
    F: FnOnce(MockProtocolController, PoolCommandSender, PoolManager) -> V,
    V: Future<Output = (MockProtocolController, PoolCommandSender, PoolManager)>,
{
    let (protocol_controller, protocol_command_sender, protocol_pool_event_receiver) =
        MockProtocolController::new();

    let (pool_command_sender, pool_manager) = pool_controller::start_pool_controller(
        cfg,
        protocol_command_sender,
        protocol_pool_event_receiver,
    )
    .await
    .unwrap();

    let (_protocol_controller, _pool_command_sender, pool_manager) =
        test(protocol_controller, pool_command_sender, pool_manager).await;

    pool_manager.stop().await.unwrap();
}

pub fn get_transaction(expire_period: u64, fee: u64) -> (Signed<Operation, OperationId>, u8) {
    let sender_priv = generate_random_private_key();
    let sender_pub = derive_public_key(&sender_priv);

    let recv_priv = generate_random_private_key();
    let recv_pub = derive_public_key(&recv_priv);

    let op = OperationType::Transaction {
        recipient_address: Address::from_public_key(&recv_pub),
        amount: Amount::default(),
    };
    let content = Operation {
        fee: Amount::from_str(&fee.to_string()).unwrap(),
        op,
        sender_public_key: sender_pub,
        expire_period,
    };
    (
        Signed::new_signed(content, &sender_priv).unwrap().1,
        Address::from_public_key(&sender_pub).get_thread(2),
    )
}

/// Creates an endorsement for use in pool tests.
pub fn create_endorsement(slot: Slot) -> Signed<Endorsement, EndorsementId> {
    let sender_priv = generate_random_private_key();
    let sender_public_key = derive_public_key(&sender_priv);

    let content = Endorsement {
        sender_public_key,
        slot,
        index: 0,
        endorsed_block: BlockId(Hash::compute_from("blabla".as_bytes())),
    };
    Signed::new_signed(content, &sender_priv).unwrap().1
}

pub fn get_transaction_with_addresses(
    expire_period: u64,
    fee: u64,
    sender_pub: PublicKey,
    sender_priv: PrivateKey,
    recv_pub: PublicKey,
) -> (Signed<Operation, OperationId>, u8) {
    let op = OperationType::Transaction {
        recipient_address: Address::from_public_key(&recv_pub),
        amount: Amount::default(),
    };
    let content = Operation {
        fee: Amount::from_str(&fee.to_string()).unwrap(),
        op,
        sender_public_key: sender_pub,
        expire_period,
    };
    (
        Signed::new_signed(content, &sender_priv).unwrap().1,
        Address::from_public_key(&sender_pub).get_thread(2),
    )
}

pub fn create_executesc(
    expire_period: u64,
    fee: u64,
    max_gas: u64,
    gas_price: u64,
) -> (Signed<Operation, OperationId>, u8) {
    let priv_key = generate_random_private_key();
    let sender_public_key = derive_public_key(&priv_key);

    let data = vec![42; 7];
    let coins = 0_u64;

    let op = OperationType::ExecuteSC {
        data,
        max_gas,
        coins: Amount::from_str(&coins.to_string()).unwrap(),
        gas_price: Amount::from_str(&gas_price.to_string()).unwrap(),
    };

    let content = Operation {
        sender_public_key,
        fee: Amount::from_str(&fee.to_string()).unwrap(),
        expire_period,
        op,
    };

    (
        Signed::new_signed(content, &priv_key).unwrap().1,
        Address::from_public_key(&sender_public_key).get_thread(2),
    )
}
