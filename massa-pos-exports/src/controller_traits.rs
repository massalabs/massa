// Copyright (c) 2022 MASSA LABS <info@massa.net>

//! This module exports generic traits representing interfaces for interacting
//! with the PoS selector worker.

use crate::PosResult;
use crate::Selection;
use anyhow::Result;
use massa_hash::Hash;
use massa_models::api::IndexedSlot;
use massa_models::prehash::Map;
use massa_models::Address;
use massa_models::Slot;

/// interface that communicates with the selector worker thread
pub trait SelectorController: Send + Sync {
    /// Raits for draws to reach at least a given cycle number.
    /// Returns the latest cycle number reached (can be higher than `cycle`).
    /// Errors can occur if the thread stopped.
    fn wait_for_draws(&mut self, cycle: u64) -> PosResult<u64>;

    /// Feed cycle to the selector
    ///
    /// # Arguments
    /// * `cycle`: cycle number to be drawn
    /// * `lookback_rolls`: lookback rolls used for the draw (cycle - 3)
    /// * `lookback_seed`: lookback seed hash for the draw (cycle - 2)
    fn feed_cycle(&self, cycle: u64, lookback_rolls: Map<Address, u64>, lookback_seed: Hash);

    /// Get [Selection] computed for a slot:
    /// # Arguments
    /// * `slot`: target slot of the selection
    fn get_selection(&self, slot: Slot) -> Result<Selection>;

    /// Return a list of slots where `address` has been choosen to produce a
    /// block and a list where he is choosen for the endorsements.
    /// Look from the `start` slot to the `end` slot.
    fn get_address_selections(
        &self,
        address: &Address,
        start: Slot,
        end: Slot,
    ) -> (Vec<Slot>, Vec<IndexedSlot>);

    /// Get [Address] of the selected block producer for a given slot
    /// # Arguments
    /// * `slot`: target slot of the selection
    fn get_producer(&self, slot: Slot) -> Result<Address>;

    /// Returns a boxed clone of self.
    /// Useful to allow cloning `Box<dyn SelectorController>`.
    fn clone_box(&self) -> Box<dyn SelectorController>;
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
