// Copyright (c) 2022 MASSA LABS <info@massa.net>

use crate::{operation_pool::OperationPool, start_pool_controller};
use massa_execution_exports::test_exports::{
    MockExecutionController, MockExecutionControllerMessage,
};
use massa_hash::Hash;
use massa_models::{
    address::Address,
    amount::Amount,
    block::BlockId,
    endorsement::{Endorsement, EndorsementSerializer, WrappedEndorsement},
    operation::{Operation, OperationSerializer, OperationType, WrappedOperation},
    slot::Slot,
    wrapped::WrappedContent,
};
use massa_pool_exports::{PoolConfig, PoolController, PoolManager};
use massa_signature::{KeyPair, PublicKey};
use massa_storage::Storage;
use std::collections::BTreeMap;
use std::str::FromStr;
use std::sync::mpsc::Receiver;

/// Tooling to create a transaction with an expire periods
/// TODO move tooling in a dedicated module
pub fn create_operation_with_expire_period(
    keypair: &KeyPair,
    expire_period: u64,
) -> WrappedOperation {
    let recv_keypair = KeyPair::generate();

    let op = OperationType::Transaction {
        recipient_address: Address::from_public_key(&recv_keypair.get_public_key()),
        amount: Amount::default(),
    };
    let content = Operation {
        fee: Amount::default(),
        op,
        expire_period,
    };
    Operation::new_wrapped(content, OperationSerializer::new(), keypair).unwrap()
}

/// Return `n` wrapped operations
pub fn create_some_operations(
    n: usize,
    keypair: &KeyPair,
    expire_period: u64,
) -> Vec<WrappedOperation> {
    (0..n)
        .map(|_| create_operation_with_expire_period(keypair, expire_period))
        .collect()
}

pub fn pool_test<F>(cfg: PoolConfig, test: F)
where
    F: FnOnce(
        Box<dyn PoolManager>,
        Box<dyn PoolController>,
        Receiver<MockExecutionControllerMessage>,
        Storage,
    ),
{
    let storage: Storage = Storage::create_root();

    let (execution_controller, execution_receiver) = MockExecutionController::new_with_receiver();
    let (pool_manager, pool_controller) =
        start_pool_controller(cfg, &storage, execution_controller);

    test(pool_manager, pool_controller, execution_receiver, storage)
}

pub fn operation_pool_test<F>(cfg: PoolConfig, test: F)
where
    F: FnOnce(OperationPool, Storage),
{
    let (execution_controller, _) = MockExecutionController::new_with_receiver();
    let storage = Storage::create_root();
    test(
        OperationPool::init(cfg, &storage.clone_without_refs(), execution_controller),
        storage,
    )
}

pub fn _get_transaction(expire_period: u64, fee: u64) -> WrappedOperation {
    let sender_keypair = KeyPair::generate();

    let op = OperationType::Transaction {
        recipient_address: Address::from_public_key(&KeyPair::generate().get_public_key()),
        amount: Amount::default(),
    };
    let content = Operation {
        fee: Amount::from_str(&fee.to_string()).unwrap(),
        op,
        expire_period,
    };
    Operation::new_wrapped(content, OperationSerializer::new(), &sender_keypair).unwrap()
}

/// Creates an endorsement for use in pool tests.
pub fn _create_endorsement(slot: Slot) -> WrappedEndorsement {
    let sender_keypair = KeyPair::generate();

    let content = Endorsement {
        slot,
        index: 0,
        endorsed_block: BlockId(Hash::compute_from("blabla".as_bytes())),
    };
    Endorsement::new_wrapped(content, EndorsementSerializer::new(), &sender_keypair).unwrap()
}

pub fn _get_transaction_with_addresses(
    expire_period: u64,
    fee: u64,
    sender_keypair: &KeyPair,
    recv_pub: PublicKey,
) -> WrappedOperation {
    let op = OperationType::Transaction {
        recipient_address: Address::from_public_key(&recv_pub),
        amount: Amount::default(),
    };
    let content = Operation {
        fee: Amount::from_str(&fee.to_string()).unwrap(),
        op,
        expire_period,
    };
    Operation::new_wrapped(content, OperationSerializer::new(), sender_keypair).unwrap()
}
