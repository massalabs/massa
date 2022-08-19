use std::{
    sync::{Arc, RwLock},
    thread::sleep,
    time::Duration,
};

use massa_consensus_exports::{commands::ConsensusCommand, test_exports::MockConsensusController};
use massa_factory_exports::{test_exports::create_empty_block, FactoryChannels, FactoryConfig};
use massa_models::{
    constants::ENDORSEMENT_COUNT, prehash::Map, test_exports::get_next_slot_instant, Address, Slot,
};
use massa_pool_exports::test_exports::{MockPoolController, MockPoolControllerMessage};
use massa_pos_exports::{
    test_exports::{MockSelectorController, MockSelectorControllerMessage},
    Selection,
};
use massa_signature::KeyPair;
use massa_storage::Storage;
use massa_time::MassaTime;
use serial_test::serial;

use crate::start_factory;
use massa_wallet::test_exports::create_test_wallet;

#[test]
#[serial]
fn basic_creation() {
    let (selector_controller, selector_receiver) = MockSelectorController::new_with_receiver();
    let (mut consensus_controller, consensus_command_sender, _consensus_event_receiver) =
        MockConsensusController::new_with_receiver();
    let (pool_controller, pool_receiver) = MockPoolController::new_with_receiver();
    let mut storage = Storage::default();
    let mut factory_config = FactoryConfig::default();

    let producer_keypair = KeyPair::generate();
    let producer_address = Address::from_public_key(&producer_keypair.get_public_key());
    let mut accounts = Map::default();

    let mut genesis_blocks = vec![];
    for i in 0..factory_config.thread_count {
        let block = create_empty_block(&producer_keypair, &Slot::new(0, i));
        genesis_blocks.push((block.id, 0));
        storage.store_block(block);
    }

    accounts.insert(producer_address, producer_keypair);
    factory_config.t0 = factory_config.t0.checked_div_u64(8).unwrap();
    factory_config.genesis_timestamp = factory_config
        .genesis_timestamp
        .checked_sub(factory_config.t0)
        .unwrap();
    let mut factory_manager = start_factory(
        factory_config.clone(),
        Arc::new(RwLock::new(create_test_wallet(Some(accounts)))),
        FactoryChannels {
            selector: selector_controller,
            consensus: consensus_command_sender,
            pool: pool_controller,
            storage: storage.clone_without_refs(),
        },
    );
    let now = MassaTime::now().expect("could not get current time");
    let next_slot_instant = get_next_slot_instant(
        factory_config.genesis_timestamp,
        factory_config.thread_count,
        factory_config.t0,
    );
    sleep(dbg!(next_slot_instant
        .checked_sub(now)
        .unwrap()
        .to_duration()));
    loop {
        match selector_receiver.recv_timeout(Duration::from_millis(100)) {
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
    match consensus_controller
        .consensus_command_rx
        .blocking_recv()
        .unwrap()
    {
        ConsensusCommand::GetBestParents { response_tx } => {
            response_tx.send(genesis_blocks).unwrap();
        }
        _ => panic!("unexpected message"),
    }
    match pool_receiver
        .recv_timeout(Duration::from_millis(100))
        .unwrap()
    {
        MockPoolControllerMessage::GetBlockEndorsements {
            block_id: _,
            slot: _,
            response_tx,
        } => {
            response_tx.send((vec![], Storage::default())).unwrap();
        }
        _ => panic!("unexpected message"),
    }
    match pool_receiver
        .recv_timeout(Duration::from_millis(100))
        .unwrap()
    {
        MockPoolControllerMessage::GetBlockOperations {
            slot: _,
            response_tx,
        } => {
            response_tx.send((vec![], Storage::default())).unwrap();
        }
        _ => panic!("unexpected message"),
    }
    match consensus_controller
        .consensus_command_rx
        .blocking_recv()
        .unwrap()
    {
        ConsensusCommand::SendBlock {
            block_id,
            block_storage,
            response_tx,
        } => {
            dbg!(block_storage.retrieve_block(&block_id));
            response_tx.send(()).unwrap();
        }
        _ => panic!("unexpected message"),
    }
    factory_manager.stop();
}
