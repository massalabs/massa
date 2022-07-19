// Copyright (c) 2022 MASSA LABS <info@massa.net>

//! This module implements a selector controller.
//! See `massa-pos-exports/controller_traits.rs` for functional details.

use std::sync::{atomic::AtomicBool, Arc};

use anyhow::{bail, Result};
use massa_models::{api::IndexedSlot, Address, Slot};

use massa_pos_exports::{CycleInfo, PosResult, Selection, SelectorController, SelectorManager};
use tracing::{info, warn};

use crate::{DrawCachePtr, InputDataPtr};

#[derive(Clone)]
/// implementation of the selector controller
pub struct SelectorControllerImpl {
    /// todo: use a config structure
    pub(crate) periods_per_cycle: u64,
    /// Store in cache the computed selections for each cycle.
    pub(crate) cache: DrawCachePtr,
    /// Continuously
    pub(crate) input_data: InputDataPtr,
    /// thread count
    pub(crate) thread_count: u8,
}

impl SelectorController for SelectorControllerImpl {
    /// Feed cycle to the selector
    ///
    /// # Arguments
    /// * `cycle_info`: give or regive a cycle info for a background
    ///                 computation of the draws.
    fn feed_cycle(&self, cycle_info: CycleInfo) {
        self.input_data.1.lock().push_back(cycle_info);
        self.input_data.0.notify_one();
    }

    /// Get [Selection] computed for a slot:
    /// # Arguments
    /// * `slot`: target slot of the selection
    fn get_selection(&self, slot: Slot) -> Result<Selection> {
        match self
            .cache
            .read()
            .get(&slot.get_cycle(self.periods_per_cycle))
            .and_then(|selections| selections.get(&slot))
        {
            Some(selection) => Ok(selection.clone()),
            None => bail!("error: selection not found for slot {}", slot),
        }
    }

    /// Get [Address] of the selected block producer for a given slot
    /// # Arguments
    /// * `slot`: target slot of the selection
    fn get_producer(&self, slot: Slot) -> Result<Address> {
        let selection = self.get_selection(slot)?;
        Ok(selection.producer)
    }

    /// Return a list of slots where `address` has been choosen to produce a
    /// block and a list where he is choosen for the endorsements.
    /// Look from the `start` slot to the `end` slot.
    fn filter_selection_by_address(
        &self,
        address: &Address,
        mut slot: Slot, /* starting slot */
        end: Slot,
    ) -> (Vec<Slot>, Vec<IndexedSlot>) {
        let cache = self.cache.read();
        let mut slot_producers = vec![];
        let mut slot_endorsers = vec![];
        while slot < end {
            if let Some(selection) = cache
                .get(&slot.get_cycle(self.periods_per_cycle))
                .and_then(|selections| selections.get(&slot))
            {
                if selection.producer == *address {
                    slot_producers.push(slot);
                } else if let Some(index) = selection.endorsments.iter().position(|e| e == address)
                {
                    slot_endorsers.push(IndexedSlot { slot, index });
                }
            }
            slot = match slot.get_next_slot(self.thread_count) {
                Ok(next_slot) => next_slot,
                _ => break,
            };
        }
        (slot_producers, slot_endorsers)
    }

    /// Returns a boxed clone of self.
    /// Allows cloning `Box<dyn SelectorController>`,
    /// see `massa-pos-exports/controller_traits.rs`
    fn clone_box(&self) -> Box<dyn SelectorController> {
        Box::new(self.clone())
    }
}

/// Implementation of the Selector manager
/// Allows stopping the selector worker
pub struct SelectorManagerImpl {
    // todo: message passing may be enough to signal to the
    //       thread to stop.
    /// handle used to join the worker thread
    pub(crate) thread_handle: Option<std::thread::JoinHandle<PosResult<()>>>,
    pub(crate) stop_flag: Arc<AtomicBool>,
}

impl SelectorManager for SelectorManagerImpl {
    /// stops the worker
    fn stop(&mut self) {
        info!("stopping selector worker...");
        self.stop_flag
            .store(true, std::sync::atomic::Ordering::Relaxed);
        // join the selector thread
        if let Some(join_handle) = self.thread_handle.take() {
            if let Err(err) = join_handle
                .join()
                .expect("selector thread panicked on try to join")
            {
                warn!("{}", err);
            }
        }
        info!("selector worker stopped");
    }
}
