use std::{sync::Arc, time::Duration};

use massa_consensus_exports::MockConsensusControllerWrapper;
use massa_final_state::MockFinalStateController;
use massa_metrics::MassaMetrics;
use massa_models::config::THREAD_COUNT;
use massa_protocol_exports::MockProtocolControllerWrapper;
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
    pub consensus_controller: MockConsensusControllerWrapper,
    pub protocol_controller: MockProtocolControllerWrapper,
    pub listener: MockBootstrapTcpListener,
    pub server_keypair: KeyPair,
}

impl BootstrapServerForeignControllers {
    pub fn new_with_mocks() -> Self {
        Self {
            final_state_controller: Arc::new(RwLock::new(MockFinalStateController::new())),
            consensus_controller: MockConsensusControllerWrapper::new(),
            protocol_controller: MockProtocolControllerWrapper::new(),
            listener: MockBootstrapTcpListener::new(),
            server_keypair: KeyPair::generate(0).unwrap(),
        }
    }
}

pub struct BootstrapServerTestUniverse {
    pub module_manager: Option<Box<BootstrapManager>>,
}

impl TestUniverse for BootstrapServerTestUniverse {
    type Config = BootstrapConfig;
    type ForeignControllers = BootstrapServerForeignControllers;

    fn new(controllers: Self::ForeignControllers, config: Self::Config) -> Self {
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
            controllers.server_keypair,
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
            module_manager: Some(Box::new(module_manager)),
        };
        universe.initialize();
        universe
    }
}

impl Drop for BootstrapServerTestUniverse {
    fn drop(&mut self) {
        self.module_manager.take().unwrap().stop().unwrap();
    }
}
