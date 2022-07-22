// Copyright (c) 2022 MASSA LABS <info@massa.net>

use std::thread::JoinHandle;

use massa_factory_exports::{FactoryConfig, FactoryController, FactoryManager, FactoryResult};

use crate::controller::{FactoryControllerImpl, FactoryManagerImpl};

/// Structure gathering all elements needed by the factory thread
pub(crate) struct FactoryThread;

impl FactoryThread {
    /// Creates the `FactoryThread` structure to gather all data and references
    /// needed by the factory worker thread.
    pub(crate) fn spawn() -> JoinHandle<FactoryResult<()>> {
        std::thread::spawn(|| {
            let this = Self {};
            this.run()
        })
    }

    fn run(self) -> FactoryResult<()> {
        // todo: mut self
        todo!()
    }
}

/// Launches factory worker thread and returns a pair to interact with it.
///
/// # parameters
/// * none
///
/// # Returns
/// A pair `(factory_manager, factory_controller)` where:
/// * `factory_manager`: allows to stop the worker
/// * `factory_controller`: allows sending requests and notifications to the worker
pub fn start_factory_worker(
    _: FactoryConfig,
) -> (Box<dyn FactoryManager>, Box<dyn FactoryController>) {
    let controller = FactoryControllerImpl {};

    // launch the factory thread
    let thread_handle = FactoryThread::spawn();
    let manager = FactoryManagerImpl {
        _thread_handle: Some(thread_handle),
    };
    (Box::new(manager), Box::new(controller))
}
