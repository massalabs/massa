use std::time::Duration;

use massa_channel::MassaChannel;
use massa_consensus_exports::{
    ConsensusBroadcasts, ConsensusChannels, ConsensusConfig, ConsensusController,
};
use massa_execution_exports::MockExecutionController;
use massa_metrics::MassaMetrics;
use massa_models::config::THREAD_COUNT;
use massa_pool_exports::MockPoolController;
use massa_pos_exports::MockSelectorController;
use massa_protocol_exports::MockProtocolController;
use massa_storage::Storage;
use massa_test_framework::TestUniverse;

use crate::start_consensus_worker;

pub struct ConsensusForeignControllers {
    pub execution_controller: Box<MockExecutionController>,
    pub protocol_controller: Box<MockProtocolController>,
    pub pool_controller: Box<MockPoolController>,
    pub selector_controller: Box<MockSelectorController>,
    pub storage: Storage,
}

impl ConsensusForeignControllers {
    pub fn new_with_mocks() -> Self {
        Self {
            execution_controller: Box::new(MockExecutionController::new()),
            protocol_controller: Box::new(MockProtocolController::new()),
            pool_controller: Box::new(MockPoolController::new()),
            selector_controller: Box::new(MockSelectorController::new()),
            storage: Storage::create_root(),
        }
    }
}

pub struct ConsensusTestUniverse {
    pub module_controller: Box<dyn ConsensusController>,
}

impl TestUniverse for ConsensusTestUniverse {
    type ForeignControllers = ConsensusForeignControllers;
    type Config = ConsensusConfig;
    fn new(mut foreign_controllers: Self::ForeignControllers, config: Self::Config) -> Self {
        foreign_controllers
            .protocol_controller
            .expect_integrated_block()
            .returning(|_, _| Ok(()));
        foreign_controllers
            .protocol_controller
            .expect_send_wishlist_delta()
            .returning(|_, _| Ok(()));
        foreign_controllers
            .protocol_controller
            .expect_notify_block_attack()
            .returning(|_| Ok(()));
        // launch consensus controller
        let (consensus_event_sender, _) =
            MassaChannel::new(String::from("consensus_event"), Some(10));

        // All API channels
        let (block_sender, _block_receiver) = tokio::sync::broadcast::channel(10);
        let (block_header_sender, _block_header_receiver) = tokio::sync::broadcast::channel(10);
        let (filled_block_sender, _filled_block_receiver) = tokio::sync::broadcast::channel(10);
        let (consensus_controller, _) = start_consensus_worker(
            config,
            ConsensusChannels {
                broadcasts: ConsensusBroadcasts {
                    block_sender,
                    block_header_sender,
                    filled_block_sender,
                },
                controller_event_tx: consensus_event_sender,
                execution_controller: foreign_controllers.execution_controller,
                protocol_controller: foreign_controllers.protocol_controller,
                pool_controller: foreign_controllers.pool_controller,
                selector_controller: foreign_controllers.selector_controller,
            },
            None,
            foreign_controllers.storage.clone(),
            MassaMetrics::new(
                false,
                "0.0.0.0:9898".parse().unwrap(),
                THREAD_COUNT,
                Duration::from_secs(1),
            )
            .0,
        );
        let universe = Self {
            module_controller: consensus_controller,
        };
        universe.initialize();
        universe
    }
}
