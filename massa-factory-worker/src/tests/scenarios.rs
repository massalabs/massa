use std::sync::{Arc, RwLock};

use massa_consensus_exports::test_exports::MockConsensusController;
use massa_factory_exports::{FactoryChannels, FactoryConfig};
use massa_pos_exports::test_exports::MockSelectorController;
use massa_storage::Storage;
use serial_test::serial;

use crate::start_factory;
use massa_wallet::test_exports::create_test_wallet;

#[test]
#[serial]
fn basic_creation() {
    let (selector_controller, selector_receiver) = MockSelectorController::new_with_receiver();
    let (consensus_controller, consensus_command_sender, consensus_event_receiver) =
        MockConsensusController::new_with_receiver();
    let storage = Storage::default();
    start_factory(
        FactoryConfig::default(),
        Arc::new(RwLock::new(create_test_wallet())),
        FactoryChannels {
            selector: selector_controller,
            consensus: consensus_command_sender,
            pool: storage,
        },
    );
}
