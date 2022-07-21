// Copyright (c) 2022 MASSA LABS <info@massa.net>

//! This module implements a factory controller.
//! See `massa-factory-exports/controller_traits.rs` for functional details.

use massa_factory_exports::{FactoryController, FactoryManager, FactoryResult, ProductionHistory};
use massa_models::{prehash::Set, Address};
use massa_signature::KeyPair;
use tracing::info;

#[derive(Clone)]
/// implementation of the factory controller
pub struct FactoryControllerImpl;

impl FactoryController for FactoryControllerImpl {
    /// Get block production history
    fn get_production_history(&self) -> ProductionHistory {
        todo!()
    }

    /// Enable or disable production
    fn set_production(&self, _enable: bool) {
        todo!()
    }

    /// Register staking keys
    fn register_staking_keys(&self, _keys: Vec<KeyPair>) -> FactoryResult<()> {
        todo!()
    }

    /// remove some keys from staking keys by associated address
    /// the node won't be able to stake with these keys anymore
    /// They will be erased from the staking keys file
    fn remove_staking_addresses(&self, _addresses: Set<Address>) -> FactoryResult<()> {
        todo!()
    }

    /// get staking addresses
    fn get_staking_addresses(&self) -> FactoryResult<Set<Address>> {
        todo!()
    }

    /// Returns a boxed clone of self.
    /// Allows cloning `Box<dyn FactoryController>`,
    /// see `massa-factory-exports/controller_traits.rs`
    fn clone_box(&self) -> Box<dyn FactoryController> {
        Box::new(self.clone())
    }
}

/// Implementation of the factory manager
/// Allows stopping the factory worker
pub struct FactoryManagerImpl {
    /// handle used to join the worker thread
    pub(crate) _thread_handle: Option<std::thread::JoinHandle<FactoryResult<()>>>,
}

impl FactoryManager for FactoryManagerImpl {
    /// stops the worker
    fn stop(&mut self) {
        info!("stopping factory worker...");
        // todo
        info!("factory worker stopped");
    }
}
