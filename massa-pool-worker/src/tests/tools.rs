// Copyright (c) 2022 MASSA LABS <info@massa.net>

use crate::{operation_pool::OperationPool, start_pool_controller};
use crossbeam_channel as _;
use massa_execution_exports::test_exports::{
    MockExecutionController, MockExecutionControllerMessage,
};
use massa_hash::Hash;
use massa_models::{
    address::Address,
    amount::Amount,
    block_id::BlockId,
    endorsement::{Endorsement, EndorsementSerializer, SecureShareEndorsement},
    operation::{Operation, OperationSerializer, OperationType, SecureShareOperation},
    secure_share::SecureShareContent,
    slot::Slot,
};
use massa_pool_exports::{PoolChannels, PoolConfig, PoolController, PoolManager};
use massa_pos_exports::test_exports::{MockSelectorController, MockSelectorControllerMessage};
use massa_signature::KeyPair;
use massa_storage::Storage;
use std::sync::mpsc::Receiver;
use tokio::sync::broadcast;

#[derive(Default)]
pub(crate) struct OpGenerator {
    creator: Option<KeyPair>,
    receiver: Option<KeyPair>,
    fee: Option<Amount>,
    amount: Option<Amount>,
    expirery: Option<u64>,
}

impl OpGenerator {
    pub(crate) fn expirery(mut self, expirery: u64) -> Self {
        self.expirery = Some(expirery);
        self
    }

    #[allow(dead_code)]
    pub(crate) fn amount(mut self, amount: Amount) -> Self {
        self.amount = Some(amount);
        self
    }

    pub(crate) fn fee(mut self, fee: Amount) -> Self {
        self.fee = Some(fee);
        self
    }

    #[allow(dead_code)]
    pub(crate) fn receiver(mut self, receiver: KeyPair) -> Self {
        self.receiver = Some(receiver);
        self
    }

    pub(crate) fn creator(mut self, creator: KeyPair) -> Self {
        self.creator = Some(creator);
        self
    }

    pub(crate) fn generate(&self) -> SecureShareOperation {
        let creator = self.creator.clone().unwrap_or_else(KeyPair::generate);
        let receiver = self.receiver.clone().unwrap_or_else(KeyPair::generate);
        let fee = self.fee.unwrap_or_default();
        let amount = self.amount.unwrap_or_default();
        let expirery = self.expirery.unwrap_or_default();

        let op = OperationType::Transaction {
            recipient_address: Address::from_public_key(&receiver.get_public_key()),
            amount,
        };
        let content = Operation {
            fee,
            op,
            expire_period: expirery,
        };
        Operation::new_verifiable(content, OperationSerializer::new(), &creator).unwrap()
    }
}

/// Return `n` signed operations
pub(crate) fn create_some_operations(n: usize, op_gen: &OpGenerator) -> Vec<SecureShareOperation> {
    (0..n).map(|_| op_gen.generate()).collect()
}

/// Creates module mocks, providing the environment needed to run the provided closure
pub fn pool_test<F>(cfg: PoolConfig, test: F)
where
    F: FnOnce(
        Box<dyn PoolManager>,
        Box<dyn PoolController>,
        Receiver<MockExecutionControllerMessage>,
        crossbeam_channel::Receiver<MockSelectorControllerMessage>,
        Storage,
    ),
{
    let storage: Storage = Storage::create_root();
    let operation_sender = broadcast::channel(5000).0;
    let (execution_controller, execution_receiver) = MockExecutionController::new_with_receiver();
    let (selector_controller, selector_receiver) = MockSelectorController::new_with_receiver();
    let (pool_manager, pool_controller) = start_pool_controller(
        cfg,
        &storage,
        execution_controller,
        PoolChannels {
            operation_sender,
            selector: selector_controller,
        },
    );

    test(
        pool_manager,
        pool_controller,
        execution_receiver,
        selector_receiver,
        storage,
    )
}

pub fn operation_pool_test<F>(cfg: PoolConfig, test: F)
where
    F: FnOnce(OperationPool, Storage),
{
    let operation_sender = broadcast::channel(5000).0;
    let (execution_controller, _) = MockExecutionController::new_with_receiver();
    let (selector_controller, _selector_receiver) = MockSelectorController::new_with_receiver();
    let storage = Storage::create_root();
    test(
        OperationPool::init(
            cfg,
            &storage.clone_without_refs(),
            execution_controller,
            PoolChannels {
                operation_sender,
                selector: selector_controller,
            },
        ),
        storage,
    )
}

/// Creates an endorsement for use in pool tests.
pub fn _create_endorsement(slot: Slot) -> SecureShareEndorsement {
    let sender_keypair = KeyPair::generate();

    let content = Endorsement {
        slot,
        index: 0,
        endorsed_block: BlockId(Hash::compute_from("blabla".as_bytes())),
    };
    Endorsement::new_verifiable(content, EndorsementSerializer::new(), &sender_keypair).unwrap()
}
