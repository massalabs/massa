use std::{
    sync::{mpsc::Receiver, Arc, RwLock},
    thread::sleep,
    time::Duration,
};

use massa_consensus_exports::{commands::ConsensusCommand, test_exports::MockConsensusController};
use massa_factory_exports::{
    test_exports::create_empty_block, FactoryChannels, FactoryConfig, FactoryManager,
};
use massa_models::{
    constants::ENDORSEMENT_COUNT, prehash::Map, test_exports::get_next_slot_instant, Address,
    BlockId, Slot, WrappedOperation, WrappedEndorsement,
};
use massa_pool_exports::test_exports::{MockPoolController, MockPoolControllerMessage};
use massa_pos_exports::{
    test_exports::{MockSelectorController, MockSelectorControllerMessage},
    Selection,
};
use massa_signature::KeyPair;
use massa_storage::Storage;
use massa_time::MassaTime;

use crate::start_factory;
use massa_wallet::test_exports::create_test_wallet;

pub struct TestFactory {
    consensus_controller: MockConsensusController,
    pool_receiver: Receiver<MockPoolControllerMessage>,
    selector_receiver: Receiver<MockSelectorControllerMessage>,
    factory_config: FactoryConfig,
    factory_manager: Box<dyn FactoryManager>,
    genesis_blocks: Vec<(BlockId, u64)>,
    storage: Storage,
    keypair: KeyPair,
}

impl TestFactory {
    pub fn new(default_keypair: &KeyPair) -> TestFactory {
        let (selector_controller, selector_receiver) = MockSelectorController::new_with_receiver();
        let (consensus_controller, consensus_command_sender, _consensus_event_receiver) =
            MockConsensusController::new_with_receiver();
        let (pool_controller, pool_receiver) = MockPoolController::new_with_receiver();
        let mut storage = Storage::default();
        let mut factory_config = FactoryConfig::default();

        let producer_keypair = default_keypair;
        let producer_address = Address::from_public_key(&producer_keypair.get_public_key());
        let mut accounts = Map::default();

        let mut genesis_blocks = vec![];
        for i in 0..factory_config.thread_count {
            let block = create_empty_block(&producer_keypair, &Slot::new(0, i));
            genesis_blocks.push((block.id, 0));
            storage.store_block(block);
        }

        accounts.insert(producer_address, producer_keypair.clone());
        factory_config.t0 = MassaTime::from(400);
        factory_config.genesis_timestamp = factory_config
            .genesis_timestamp
            .checked_sub(factory_config.t0)
            .unwrap();
        let factory_manager = start_factory(
            factory_config.clone(),
            Arc::new(RwLock::new(create_test_wallet(Some(accounts)))),
            FactoryChannels {
                selector: selector_controller.clone(),
                consensus: consensus_command_sender,
                pool: pool_controller.clone(),
                storage: storage.clone_without_refs(),
            },
        );

        TestFactory {
            consensus_controller,
            pool_receiver,
            selector_receiver,
            factory_config,
            factory_manager,
            genesis_blocks,
            storage,
            keypair: default_keypair.clone(),
        }
    }

    pub fn get_next_created_block(
        &mut self,
        operations: Option<Vec<WrappedOperation>>,
        endorsements: Option<Vec<WrappedEndorsement>>,
    ) -> (BlockId, Storage) {
        let now = MassaTime::now().expect("could not get current time");
        let next_slot_instant = get_next_slot_instant(
            self.factory_config.genesis_timestamp,
            self.factory_config.thread_count,
            self.factory_config.t0,
        );
        sleep(next_slot_instant
            .checked_sub(now)
            .unwrap()
            .to_duration());
        let producer_address = Address::from_public_key(&self.keypair.get_public_key());
        loop {
            match self
                .selector_receiver
                .recv_timeout(Duration::from_millis(100))
            {
                Ok(MockSelectorControllerMessage::GetProducer {
                    slot: _,
                    response_tx,
                }) => {
                    println!("test in receiver");
                    response_tx.send(Ok(producer_address)).unwrap();
                }
                Ok(MockSelectorControllerMessage::GetSelection {
                    slot: _,
                    response_tx,
                }) => {
                    println!("test in receiver2");
                    response_tx
                        .send(Ok(Selection {
                            producer: producer_address,
                            endorsements: vec![producer_address; ENDORSEMENT_COUNT as usize],
                        }))
                        .unwrap();
                }
                Err(_) => {
                    break;
                }
                _ => panic!("unexpected message"),
            }
        }
        match self
            .consensus_controller
            .consensus_command_rx
            .blocking_recv()
            .unwrap()
        {
            ConsensusCommand::GetBestParents { response_tx } => {
                response_tx.send(self.genesis_blocks.clone()).unwrap();
            }
            _ => panic!("unexpected message"),
        }
        match self
            .pool_receiver
            .recv_timeout(Duration::from_millis(100))
            .unwrap()
        {
            MockPoolControllerMessage::GetBlockEndorsements {
                block_id: _,
                slot: _,
                response_tx,
            } => {
                if let Some(endorsements) = endorsements {
                    let ids = endorsements.iter().map(|endo| Some(endo.id)).collect();
                    self.storage.store_endorsements(endorsements);
                    response_tx.send((ids, self.storage.clone())).unwrap();
                } else {
                    response_tx.send((vec![], Storage::default())).unwrap();
                }
            }
            _ => panic!("unexpected message"),
        }
        match self
            .pool_receiver
            .recv_timeout(Duration::from_millis(100))
            .unwrap()
        {
            MockPoolControllerMessage::GetBlockOperations {
                slot: _,
                response_tx,
            } => {
                if let Some(operations) = operations {
                    let ids = operations.iter().map(|op| op.id).collect();
                    self.storage.store_operations(operations);
                    response_tx.send((ids, self.storage.clone())).unwrap();
                } else {
                    response_tx.send((vec![], Storage::default())).unwrap();
                }
            }
            _ => panic!("unexpected message"),
        }
        match self
            .consensus_controller
            .consensus_command_rx
            .blocking_recv()
            .unwrap()
        {
            ConsensusCommand::SendBlock {
                block_id,
                block_storage,
                response_tx,
            } => {
                response_tx.send(()).unwrap();
                (block_id, block_storage)
            }
            _ => panic!("unexpected message"),
        }
    }
}

impl Drop for TestFactory {
    fn drop(&mut self) {
        self.factory_manager.stop();
    }
}
