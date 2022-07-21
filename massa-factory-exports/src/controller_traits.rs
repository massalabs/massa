// Copyright (c) 2022 MASSA LABS <info@massa.net>

//! This module exports generic traits representing interfaces for interacting
//! with the factory worker.

use massa_models::{prehash::Set, Address};
use massa_signature::KeyPair;

use crate::{FactoryResult, ProductionHistory};

/// interface that communicates with the factory worker thread
pub trait FactoryController: Send + Sync {
    /// Get the production history
    ///
    /// todo: redesign inputs
    fn get_production_history(&self) -> ProductionHistory;

    /// Enable or disable production
    fn set_production(&self, enable: bool);

    /// Register staking keys
    fn register_staking_keys(&self, keys: Vec<KeyPair>) -> FactoryResult<()>;

    /// remove some keys from staking keys by associated address
    /// the node won't be able to stake with these keys anymore
    /// They will be erased from the staking keys file
    fn remove_staking_addresses(&self, addresses: Set<Address>) -> FactoryResult<()>;

    /// get staking addresses
    fn get_staking_addresses(&self) -> FactoryResult<Set<Address>>;

    /// Returns a boxed clone of self.
    /// Useful to allow cloning `Box<dyn FactoryController>`.
    fn clone_box(&self) -> Box<dyn FactoryController>;
}

/// Allow cloning `Box<dyn FactoryController>`
/// Uses `FactoryController::clone_box` internally
impl Clone for Box<dyn FactoryController> {
    fn clone(&self) -> Box<dyn FactoryController> {
        self.clone_box()
    }
}

/// Factory manager used to stop the factory thread
pub trait FactoryManager {
    /// Stop the factory thread
    /// Note that we do not take self by value to consume it
    /// because it is not allowed to move out of Box<dyn FactoryManager>
    /// This will improve if the `unsized_fn_params` feature stabilizes enough to be safely usable.
    fn stop(&mut self);
}
