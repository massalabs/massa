use std::{sync::Arc, time::Duration};

use massa_consensus_exports::MockConsensusController;
use massa_final_state::MockFinalStateController;
use massa_metrics::MassaMetrics;
use massa_models::config::THREAD_COUNT;
use massa_protocol_exports::MockProtocolController;
use massa_signature::KeyPair;
use massa_test_framework::TestUniverse;
use mio::{Poll, Token, Waker};
use parking_lot::RwLock;

use crate::{
    listener::{BootstrapListenerStopHandle, MockBootstrapTcpListener},
    start_bootstrap_server, BootstrapConfig, BootstrapManager,
};

pub struct BootstrapServerForeignControllers {
    pub final_state_controller: Arc<RwLock<MockFinalStateController>>,
    pub consensus_controller: MockConsensusController,
    pub protocol_controller: MockProtocolController,
    pub listener: MockBootstrapTcpListener,
}

impl BootstrapServerForeignControllers {
    pub fn new_with_mocks() -> Self {
        Self {
            final_state_controller: Arc::new(RwLock::new(MockFinalStateController::new())),
            consensus_controller: MockConsensusController::new(),
            protocol_controller: MockProtocolController::new(),
            listener: MockBootstrapTcpListener::new(),
        }
    }
}

pub struct BootstrapServerTestUniverse {
    pub module_manager: Box<BootstrapManager>,
}

impl TestUniverse for BootstrapServerTestUniverse {
    type Config = BootstrapConfig;
    type ForeignControllers = BootstrapServerForeignControllers;

    fn new(controllers: Self::ForeignControllers, mut config: Self::Config) -> Self {
        let keypair = KeyPair::generate(0).unwrap();
        let version = "BOOT.1.0".parse().unwrap();
        let poll = Poll::new().unwrap();
        let waker = BootstrapListenerStopHandle(Waker::new(poll.registry(), Token(10)).unwrap());
        let module_manager = start_bootstrap_server(
            controllers.listener,
            waker,
            Box::new(controllers.consensus_controller),
            Box::new(controllers.protocol_controller),
            controllers.final_state_controller,
            config,
            keypair,
            version,
            MassaMetrics::new(
                false,
                "0.0.0.0:31248".parse().unwrap(),
                THREAD_COUNT,
                Duration::from_secs(5),
            )
            .0,
        )
        .unwrap();
        let universe = Self {
            module_manager: Box::new(module_manager),
        };
        universe.initialize();
        universe
    }
}

impl Drop for BootstrapServerTestUniverse {
    fn drop(&mut self) {
        //TODO: stop
        //self.module_manager.stop().unwrap()
    }
}
