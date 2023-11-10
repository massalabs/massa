use std::{
    sync::{Arc, Condvar, Mutex},
    time::Duration,
};

use massa_final_state::MockFinalStateController;
use massa_metrics::MassaMetrics;
use massa_models::config::THREAD_COUNT;
use massa_test_framework::TestUniverse;
use massa_time::MassaTime;
use parking_lot::RwLock;

use crate::{client::MockBSConnector, get_state, BootstrapConfig};

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
    pub interrupt: Arc<(Mutex<bool>, Condvar)>,
}

impl TestUniverse for BootstrapClientTestUniverse {
    type Config = BootstrapConfig;
    type ForeignControllers = BootstrapClientForeignControllers;

    fn new(controllers: Self::ForeignControllers, config: Self::Config) -> Self {
        let version = "BOOT.1.0".parse().unwrap();
        let interrupt = Arc::new((Mutex::new(false), Condvar::new()));
        let interrupt_clone = interrupt.clone();
        std::thread::Builder::new()
            .spawn(move || {
                get_state(
                    &config,
                    controllers.final_state_controller,
                    controllers.bs_connector,
                    version,
                    MassaTime::now().saturating_sub(MassaTime::from_millis(1000)),
                    None,
                    None,
                    interrupt_clone,
                    MassaMetrics::new(
                        false,
                        "0.0.0.0:31248".parse().unwrap(),
                        THREAD_COUNT,
                        Duration::from_secs(5),
                    )
                    .0,
                )
            })
            .unwrap();
        let universe = Self { interrupt };
        universe.initialize();
        universe
    }
}

impl BootstrapClientTestUniverse {
    pub fn stop(&mut self) {
        println!("Dropping BootstrapClientTestUniverse");
        let (lock, cvar) = &*self.interrupt;
        let mut started = lock.lock().unwrap();
        *started = true;
        // We notify the condvar that the value has changed.
        cvar.notify_one();
    }
}
