use std::{
    sync::{Arc, RwLock},
    thread::sleep,
    time::Duration,
};

use massa_consensus_exports::test_exports::MockConsensusController;
use massa_factory_exports::{FactoryChannels, FactoryConfig};
use massa_models::{test_exports::get_next_slot_instant, Slot};
use massa_pool_exports::test_exports::MockPoolController;
use massa_pos_exports::test_exports::MockSelectorController;
use massa_storage::Storage;
use massa_time::MassaTime;
use serial_test::serial;

use crate::start_factory;
use massa_wallet::test_exports::create_test_wallet;

#[test]
#[serial]
fn basic_creation() {
    let (selector_controller, selector_receiver) = MockSelectorController::new_with_receiver();
    let (consensus_controller, consensus_command_sender, consensus_event_receiver) =
        MockConsensusController::new_with_receiver();
    let (pool_controller, pool_receiver) = MockPoolController::new_with_receiver();
    let storage = Storage::default();
    let mut factory_config = FactoryConfig::default();
    factory_config.t0 = factory_config.t0.checked_div_u64(8).unwrap();
    let factory_manager = start_factory(
        factory_config.clone(),
        Arc::new(RwLock::new(create_test_wallet())),
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
    sleep(
        dbg!(next_slot_instant.checked_sub(now))
            .unwrap()
            .to_duration(),
    );
    sleep(Duration::from_secs(1));
    dbg!(storage
        .get_block_indexes()
        .read()
        .get_blocks_by_slot(Slot::new(0, 0))
        .unwrap());
}
