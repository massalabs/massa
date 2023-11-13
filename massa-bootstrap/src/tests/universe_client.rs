use std::sync::Arc;

use massa_final_state::MockFinalStateController;
use massa_models::streaming_step::StreamingStep;
use massa_test_framework::TestUniverse;
use parking_lot::RwLock;

use crate::{
    client::{bootstrap_from_server, connect_to_server, MockBSConnector},
    BootstrapClientMessage, BootstrapConfig, BootstrapError, GlobalBootstrapState,
};

pub struct BootstrapClientForeignControllers {
    pub final_state_controller: Arc<RwLock<MockFinalStateController>>,
    pub bs_connector: MockBSConnector,
}

impl BootstrapClientForeignControllers {
    pub fn new_with_mocks() -> Self {
        Self {
            final_state_controller: Arc::new(RwLock::new(MockFinalStateController::new())),
            bs_connector: MockBSConnector::new(),
        }
    }
}

pub struct BootstrapClientTestUniverse {
    controllers: BootstrapClientForeignControllers,
    config: BootstrapConfig,
    pub(crate) global_bootstrap_state: GlobalBootstrapState,
}

impl TestUniverse for BootstrapClientTestUniverse {
    type Config = BootstrapConfig;
    type ForeignControllers = BootstrapClientForeignControllers;

    fn new(controllers: Self::ForeignControllers, config: Self::Config) -> Self {
        let global_bootstrap_state =
            GlobalBootstrapState::new(controllers.final_state_controller.clone());
        let universe = Self {
            controllers,
            config,
            global_bootstrap_state,
        };
        universe.initialize();
        universe
    }
}

impl BootstrapClientTestUniverse {
    pub fn launch_bootstrap(&mut self) -> Result<(), BootstrapError> {
        //TODO: Maybe move it out of this
        let version = "BOOT.1.0".parse().unwrap();
        let mut next_bootstrap_message: BootstrapClientMessage =
            BootstrapClientMessage::AskBootstrapPart {
                last_slot: None,
                last_state_step: StreamingStep::Started,
                last_versioning_step: StreamingStep::Started,
                last_consensus_step: StreamingStep::Started,
                send_last_start_period: true,
            };

        let mut conn = connect_to_server(
            &mut self.controllers.bs_connector,
            &self.config,
            &self.config.bootstrap_list[0].0,
            &self.config.bootstrap_list[0].1.get_public_key(),
            Some(self.config.rate_limit),
        )
        .unwrap();
        bootstrap_from_server(
            &self.config,
            &mut conn,
            &mut next_bootstrap_message,
            &mut self.global_bootstrap_state,
            version,
        )
    }
}
