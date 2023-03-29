// Copyright (c) 2022 MASSA LABS <info@massa.net>

//! This module exports generic traits representing interfaces for interacting
//! with the PoS selector worker.

use std::collections::BTreeMap;

use crate::PosResult;
use massa_hash::Hash;
use massa_models::{
    address::Address,
    slot::{IndexedSlot, Slot},
};

#[cfg(feature = "testing")]
use std::collections::{HashMap, VecDeque};

/// Selections of endorsements and producer
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Selection {
    /// Chosen endorsements
    pub endorsements: Vec<Address>,
    /// Chosen block producer
    pub producer: Address,
}

/// interface that communicates with the selector worker thread
pub trait SelectorController: Send + Sync {
    /// Waits for draws to reach at least a given cycle number.
    /// Returns the latest cycle number reached (can be higher than `cycle`).
    /// Errors can occur if the thread stopped.
    fn wait_for_draws(&self, cycle: u64) -> PosResult<u64>;

    /// Feed cycle to the selector
    ///
    /// # Arguments
    /// * `cycle`: cycle number to be drawn
    /// * `lookback_rolls`: look back rolls used for the draw (cycle - 3)
    /// * `lookback_seed`: look back seed hash for the draw (cycle - 2)
    fn feed_cycle(
        &self,
        cycle: u64,
        lookback_rolls: BTreeMap<Address, u64>,
        lookback_seed: Hash,
    ) -> PosResult<()>;

    /// Get [Selection] computed for a slot:
    /// # Arguments
    /// * `slot`: target slot of the selection
    fn get_selection(&self, slot: Slot) -> PosResult<Selection>;

    /// Return a list of slots where `address` has been chosen to produce a
    /// block and a list where he is chosen for the endorsements.
    /// Look from the `start` slot to the `end` slot.
    fn get_address_selections(
        &self,
        address: &Address,
        start: Slot,
        end: Slot,
    ) -> PosResult<(Vec<Slot>, Vec<IndexedSlot>)>;

    /// Get [Address] of the selected block producer for a given slot
    /// # Arguments
    /// * `slot`: target slot of the selection
    fn get_producer(&self, slot: Slot) -> PosResult<Address>;

    /// Returns a boxed clone of self.
    /// Useful to allow cloning `Box<dyn SelectorController>`.
    fn clone_box(&self) -> Box<dyn SelectorController>;

    /// Get every [Selection]
    ///
    /// Only used in tests for post-bootstrap selection matching.
    #[cfg(feature = "testing")]
    fn get_entire_selection(&self) -> VecDeque<(u64, HashMap<Slot, Selection>)> {
        unimplemented!("mock implementation only")
    }
}

/// Allow cloning `Box<dyn SelectorController>`
/// Uses `ExecutionController::clone_box` internally
impl Clone for Box<dyn SelectorController> {
    fn clone(&self) -> Box<dyn SelectorController> {
        self.clone_box()
    }
}

/// Selector manager used to stop the selector thread
pub trait SelectorManager {
    /// Stop the selector thread
    /// Note that we do not take self by value to consume it
    /// because it is not allowed to move out of Box<dyn SelectorManager>
    /// This will improve if the `unsized_fn_params` feature stabilizes enough to be safely usable.
    fn stop(&mut self);
}
