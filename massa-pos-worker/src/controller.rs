// Copyright (c) 2022 MASSA LABS <info@massa.net>

//! This module implements a selector controller.
//! See `massa-pos-exports/controller_traits.rs` for functional details.

use std::collections::HashMap;
use std::sync::Arc;

use anyhow::{bail, Result};
use massa_models::{Address, Slot};
use parking_lot::RwLock;

use massa_pos_exports::{CycleInfo, Selection, SelectorController, SelectorManager};
use tracing::info;

use crate::InputDataPtr;

#[derive(Clone)]
/// implementation of the selector controller
pub struct SelectorControllerImpl {
    /// todo: use a config structure
    pub(crate) periods_per_cycle: u64,
    /// Store in cache the computed selections for each cycle.
    pub(crate) cache: Arc<RwLock<HashMap<u64, Vec<Selection>>>>,
    /// Continuously
    pub(crate) input_data: InputDataPtr,
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
            .and_then(|selections| selections.get(0))
        {
            Some(selection) => Ok(selection.clone()),
            None => bail!("error: selection not found for slot {}", slot),
        }
    }

    /// Get [Address] of the selected block producer for a given slot
    /// # Arguments
    /// * `slot`: target slot of the selection
    fn get_producer(&self, _slot: Slot) -> Result<Address> {
        todo!("")
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
    pub(crate) thread_handle: Option<std::thread::JoinHandle<()>>,
}

impl SelectorManager for SelectorManagerImpl {
    /// stops the worker
    fn stop(&mut self) {
        info!("stopping selector worker...");
        // todo: notify the worker thread to stop
        // join the selector thread
        if let Some(join_handle) = self.thread_handle.take() {
            join_handle
                .join()
                .expect("selector thread panicked on try to join");
        }
        info!("selector worker stopped");
    }
}
