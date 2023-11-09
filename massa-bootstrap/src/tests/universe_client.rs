use std::{
    net::SocketAddr,
    sync::{Arc, Condvar, Mutex},
    time::Duration,
};

use massa_final_state::MockFinalStateController;
use massa_metrics::MassaMetrics;
use massa_models::{config::THREAD_COUNT, node::NodeId};
use massa_signature::KeyPair;
use massa_test_framework::TestUniverse;
use massa_time::MassaTime;
use parking_lot::RwLock;

use crate::{client::MockBSConnector, get_state, BootstrapConfig, GlobalBootstrapState};

use super::tools::BASE_BOOTSTRAP_IP;

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
    pub global_bootstrap_state: GlobalBootstrapState,
}

impl TestUniverse for BootstrapClientTestUniverse {
    type Config = BootstrapConfig;
    type ForeignControllers = BootstrapClientForeignControllers;

    fn new(controllers: Self::ForeignControllers, mut config: Self::Config) -> Self {
        let keypair = KeyPair::generate(0).unwrap();
        let node_id = NodeId::new(keypair.get_public_key());
        config.bootstrap_list = vec![(SocketAddr::new(BASE_BOOTSTRAP_IP, 8069), node_id)];
        let version = "BOOT.1.0".parse().unwrap();
        let res = get_state(
            &config,
            controllers.final_state_controller,
            controllers.bs_connector,
            version,
            MassaTime::now().saturating_sub(MassaTime::from_millis(1000)),
            None,
            None,
            Arc::new((Mutex::new(false), Condvar::new())),
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
            global_bootstrap_state: res,
        };
        universe.initialize();
        universe
    }
}
