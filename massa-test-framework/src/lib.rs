use std::sync::{Arc, Condvar, Mutex};

use massa_hash::Hash;
use massa_models::{
    address::Address,
    amount::Amount,
    block::{Block, BlockSerializer, SecureShareBlock},
    block_header::{BlockHeader, BlockHeaderSerializer},
    block_id::BlockId,
    operation::{
        compute_operations_hash, Operation, OperationIdSerializer, OperationSerializer,
        OperationType, SecureShareOperation,
    },
    secure_share::SecureShareContent,
    slot::Slot,
};
use massa_signature::KeyPair;
use tracing_subscriber::filter::LevelFilter;

pub trait TestUniverse {
    type ForeignControllers;
    type Config: Default;

    fn new(controllers: Self::ForeignControllers, config: Self::Config) -> Self;

    fn initialize(&self) {
        let default_panic = std::panic::take_hook();
        std::panic::set_hook(Box::new(move |info| {
            default_panic(info);
            std::process::exit(1);
        }));
        use tracing_subscriber::prelude::*;
        let tracing_layer = tracing_subscriber::fmt::layer().with_filter(LevelFilter::DEBUG);
        tracing_subscriber::registry().with(tracing_layer).init();
    }

    fn create_block(
        keypair: &KeyPair,
        slot: Slot,
        operations: Option<Vec<SecureShareOperation>>,
    ) -> SecureShareBlock {
        let op_ids = operations
            .unwrap_or_default()
            .iter()
            .map(|op| op.id)
            .collect::<Vec<_>>();
        let operation_merkle_root = compute_operations_hash(&op_ids, &OperationIdSerializer::new());
        let header = BlockHeader::new_verifiable(
            BlockHeader {
                current_version: 0,
                announced_version: None,
                slot,
                parents: vec![
                    BlockId::generate_from_hash(Hash::compute_from("Genesis 0".as_bytes())),
                    BlockId::generate_from_hash(Hash::compute_from("Genesis 1".as_bytes())),
                ],
                operation_merkle_root,
                endorsements: Vec::new(),
                denunciations: Vec::new(),
            },
            BlockHeaderSerializer::new(),
            keypair,
        )
        .unwrap();

        Block::new_verifiable(
            Block {
                header,
                operations: op_ids,
            },
            BlockSerializer::new(),
            keypair,
        )
        .unwrap()
    }

    fn create_operation(keypair: &KeyPair, expire_period: u64) -> SecureShareOperation {
        let recv_keypair = KeyPair::generate(0).unwrap();

        let op = OperationType::Transaction {
            recipient_address: Address::from_public_key(&recv_keypair.get_public_key()),
            amount: Amount::default(),
        };
        let content = Operation {
            fee: Amount::default(),
            op,
            expire_period,
        };
        Operation::new_verifiable(content, OperationSerializer::new(), keypair).unwrap()
    }
}

pub struct WaitPoint(Arc<WaitPointInner>);

struct WaitPointInner {
    mutex: Mutex<bool>,
    condvar: Condvar,
}

impl Default for WaitPoint {
    fn default() -> Self {
        Self::new()
    }
}

impl WaitPoint {
    pub fn new() -> Self {
        Self(Arc::new(WaitPointInner {
            mutex: Mutex::new(false),
            condvar: Condvar::new(),
        }))
    }

    pub fn get_trigger_handle(&self) -> WaitPoint {
        WaitPoint(self.0.clone())
    }

    pub fn wait(&self) {
        let mut started = self.0.mutex.lock().unwrap();
        *started = false;
        while !*started {
            started = self.0.condvar.wait(started).unwrap();
        }
    }

    pub fn trigger(&self) {
        let mut started = self.0.mutex.lock().unwrap();
        *started = true;
        // We notify the condvar that the value has changed.
        self.0.condvar.notify_one();
    }
}
