// Copyright (c) 2022 MASSA LABS <info@massa.net>

use std::sync::Arc;

use crate::{operation_pool::OperationPool, start_pool_controller};
use crossbeam_channel as _;
use massa_execution_exports::MockExecutionController;
use massa_hash::Hash;
use massa_models::{
    address::Address,
    amount::Amount,
    block_id::BlockId,
    endorsement::{Endorsement, EndorsementSerializer, SecureShareEndorsement},
    operation::{Operation, OperationSerializer, OperationType, SecureShareOperation},
    prehash::PreHashMap,
    secure_share::SecureShareContent,
    slot::Slot,
};
use massa_pool_exports::{PoolChannels, PoolConfig, PoolController, PoolManager};
use massa_pos_exports::MockSelectorController as AutoMockSelectorController;
use massa_signature::KeyPair;
use massa_storage::Storage;
use massa_wallet::test_exports::create_test_wallet;
use parking_lot::RwLock;
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
        let creator = self
            .creator
            .clone()
            .unwrap_or_else(|| KeyPair::generate(0).unwrap());
        let receiver = self
            .receiver
            .clone()
            .unwrap_or_else(|| KeyPair::generate(0).unwrap());
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

pub struct PoolTestBoilerPlate {
    pub pool_manager: Box<dyn PoolManager>,
    pub pool_controller: Box<dyn PoolController>,
    pub storage: Storage,
}
impl PoolTestBoilerPlate {
    /// Sets up a pool-system that can bu run, using the mocks-stories provided
    pub fn pool_test(
        cfg: PoolConfig,
        execution_story: Box<MockExecutionController>,
        selector_story: Box<AutoMockSelectorController>,
    ) -> Self {
        let storage: Storage = Storage::create_root();
        let keypair = KeyPair::generate(0).unwrap();
        let address = Address::from_public_key(&keypair.get_public_key());
        let mut addresses = PreHashMap::default();
        addresses.insert(address, keypair);
        let wallet = Arc::new(RwLock::new(create_test_wallet(Some(addresses))));
        let endorsement_sender = broadcast::channel(2000).0;
        let operation_sender = broadcast::channel(5000).0;
        let (pool_manager, pool_controller) = start_pool_controller(
            cfg,
            &storage,
            PoolChannels {
                execution_controller: execution_story,
                endorsement_sender,
                operation_sender,
                selector: selector_story,
            },
            wallet,
        );

        Self {
            pool_manager,
            pool_controller,
            storage,
        }
    }
}

pub fn operation_pool_test<F>(cfg: PoolConfig, test: F)
where
    F: FnOnce(OperationPool, Storage),
{
    let endorsement_sender = broadcast::channel(2000).0;
    let operation_sender = broadcast::channel(5000).0;
    let execution_controller = Box::new(MockExecutionController::new());
    let selector_controller = Box::new(AutoMockSelectorController::new());
    let storage = Storage::create_root();
    let keypair = KeyPair::generate(0).unwrap();
    let address = Address::from_public_key(&keypair.get_public_key());
    let mut addresses = PreHashMap::default();
    addresses.insert(address, keypair);
    let wallet = Arc::new(RwLock::new(create_test_wallet(Some(addresses))));
    test(
        OperationPool::init(
            cfg,
            &storage.clone_without_refs(),
            PoolChannels {
                execution_controller,
                endorsement_sender,
                operation_sender,
                selector: selector_controller,
            },
            wallet,
        ),
        storage,
    )
}

/// Creates an endorsement for use in pool tests.
pub fn _create_endorsement(slot: Slot) -> SecureShareEndorsement {
    let sender_keypair = KeyPair::generate(0).unwrap();

    let content = Endorsement {
        slot,
        index: 0,
        endorsed_block: BlockId(Hash::compute_from("blabla".as_bytes())),
    };
    Endorsement::new_verifiable(content, EndorsementSerializer::new(), &sender_keypair).unwrap()
}
