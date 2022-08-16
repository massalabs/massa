// Copyright (c) 2022 MASSA LABS <info@massa.net>

use super::mock_protocol_controller::MockProtocolController;
use crate::{controller_impl::PoolControllerImpl, start_pool};
use futures::Future;
use massa_execution_exports::test_exports::MockExecutionController;
use massa_hash::Hash;
use massa_models::{
    wrapped::WrappedContent, Address, Amount, BlockId, Endorsement, EndorsementSerializer,
    Operation, OperationSerializer, OperationType, Slot, WrappedEndorsement, WrappedOperation,
};
use massa_pool_exports::{PoolConfig, PoolController};
use massa_signature::{KeyPair, PublicKey};
use massa_storage::Storage;
use std::str::FromStr;

pub async fn pool_test<F, V>(cfg: &'static PoolConfig, test: F)
where
    F: FnOnce(MockProtocolController, Box<dyn PoolController>) -> V,
    V: Future<Output = (MockProtocolController, Box<dyn PoolController>)>,
{
    let storage: Storage = Default::default();

    let (protocol_controller, protocol_command_sender, protocol_pool_event_receiver) =
        MockProtocolController::new();

    let (execution_controller, execution_receiver) = MockExecutionController::new_with_receiver();
    let pool_controller = start_pool(*cfg, storage, execution_controller);

    let (_protocol_controller, _pool_controller) =
        test(protocol_controller, Box::new(pool_controller)).await;
}

pub fn get_transaction(expire_period: u64, fee: u64) -> WrappedOperation {
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
pub fn create_endorsement(slot: Slot) -> WrappedEndorsement {
    let sender_keypair = KeyPair::generate();

    let content = Endorsement {
        slot,
        index: 0,
        endorsed_block: BlockId(Hash::compute_from("blabla".as_bytes())),
    };
    Endorsement::new_wrapped(content, EndorsementSerializer::new(), &sender_keypair).unwrap()
}

pub fn get_transaction_with_addresses(
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

pub fn create_executesc(
    expire_period: u64,
    fee: u64,
    max_gas: u64,
    gas_price: u64,
) -> WrappedOperation {
    let keypair = KeyPair::generate();

    let data = vec![42; 7];
    let coins = 0_u64;

    let op = OperationType::ExecuteSC {
        data,
        max_gas,
        coins: Amount::from_str(&coins.to_string()).unwrap(),
        gas_price: Amount::from_str(&gas_price.to_string()).unwrap(),
    };

    let content = Operation {
        fee: Amount::from_str(&fee.to_string()).unwrap(),
        expire_period,
        op,
    };
    Operation::new_wrapped(content, OperationSerializer::new(), &keypair).unwrap()
}
