// Copyright (c) 2023 MASSA LABS <info@massa.net>

//! This module implements a selector controller.
//! See `massa-pos-exports/controller_traits.rs` for functional details.

use std::collections::BTreeMap;

use crate::{Command, DrawCachePtr};
use massa_hash::Hash;
use massa_models::{
    address::Address,
    slot::{IndexedSlot, Slot},
};
use massa_pos_exports::{PosError, PosResult, Selection, SelectorController, SelectorManager};
#[cfg(feature = "testing")]
use std::collections::{HashMap, VecDeque};
use std::sync::mpsc::SyncSender;
use tracing::{info, warn};

#[derive(Clone)]
/// implementation of the selector controller
pub(crate) struct SelectorControllerImpl {
    /// todo: use a configuration structure
    pub(crate) periods_per_cycle: u64,
    /// thread count
    pub(crate) thread_count: u8,
    /// Cache storing the computed selections for each cycle.
    pub(crate) cache: DrawCachePtr,
    /// MPSC to send commands to the selector thread
    pub(crate) input_mpsc: SyncSender<Command>,
}

impl SelectorController for SelectorControllerImpl {
    /// Waits for draws to reach at least a given cycle number.
    /// Returns the latest cycle number reached (can be higher than `cycle`).
    /// Errors can occur if the thread stopped.
    fn wait_for_draws(&self, cycle: u64) -> PosResult<u64> {
        let (cache_cv, cache_lock) = &*self.cache;
        let mut cache_guard = cache_lock.read();
        loop {
            match &*cache_guard {
                Ok(draws) => {
                    if let Some(c) = draws.0.back().map(|cd| cd.cycle) {
                        if c >= cycle {
                            return Ok(c);
                        }
                    }
                }
                Err(err) => return Err(err.clone()),
            }
            cache_cv.wait(&mut cache_guard);
        }
    }

    /// Feed cycle to the selector
    ///
    /// # Arguments
    /// * `cycle`: cycle number to be drawn
    /// * `lookback_rolls`: look back rolls used for the draw (cycle - 3)
    /// * `lookback_seed`: look back seed hash for the draw (cycle - 2)

    /// * This a non-blocking function where the worker is separate,
    /// * so the feed is queued and not applied immediately and that's
    /// * done to avoid blocking while drawing. This is because the
    /// * drawing is heavy (~70k long sequence) and may get even heavier
    /// * (~400k+) when the requirements of super majority w.r.t.
    /// * endorsements kick in.
    fn feed_cycle(
        &self,
        cycle: u64,
        lookback_rolls: BTreeMap<Address, u64>,
        lookback_seed: Hash,
    ) -> PosResult<()> {
        // check status
        {
            let (_cache_cv, cache_lock) = &*self.cache;
            cache_lock.read().as_ref().map_err(|err| err.clone())?;
        }

        // send command
        self.input_mpsc
            .send(Command::DrawInput {
                cycle,
                lookback_rolls,
                lookback_seed,
            })
            .map_err(|_err| {
                PosError::ChannelDown(
                    "could not feed cycle to selector worker through channel".into(),
                )
            })?;

        Ok(())
    }

    /// Get [Selection] computed for a slot:
    /// # Arguments
    /// * `slot`: target slot of the selection
    fn get_selection(&self, slot: Slot) -> PosResult<Selection> {
        let cycle = slot.get_cycle(self.periods_per_cycle);
        let (_cache_cv, cache_lock) = &*self.cache;
        let cache_guard = cache_lock.read();
        let cache = cache_guard.as_ref().map_err(|err| err.clone())?;
        cache
            .get(cycle)
            .and_then(|selections| selections.draws.get(&slot).cloned())
            .ok_or(PosError::CycleUnavailable(cycle))
    }

    /// Get [Address] of the selected block producer for a given slot
    /// # Arguments
    /// * `slot`: target slot of the selection
    fn get_producer(&self, slot: Slot) -> PosResult<Address> {
        let cycle = slot.get_cycle(self.periods_per_cycle);
        let (_cache_cv, cache_lock) = &*self.cache;
        let cache_guard = cache_lock.read();
        let cache = cache_guard.as_ref().map_err(|err| err.clone())?;

        cache
            .get(cycle)
            .and_then(|selections| selections.draws.get(&slot).map(|s| s.producer))
            .ok_or(PosError::CycleUnavailable(cycle))
    }

    /// Return a list of slots where `address` has been chosen to produce a
    /// block and a list where he is chosen for the endorsements.
    /// Look from the `start` slot to the `end` slot.
    fn get_address_selections(
        &self,
        address: &Address,
        mut slot: Slot, /* starting slot */
        end: Slot,
    ) -> PosResult<(Vec<Slot>, Vec<IndexedSlot>)> {
        let (_cache_cv, cache_lock) = &*self.cache;
        let cache_guard = cache_lock.read();
        let cache = cache_guard.as_ref().map_err(|err| err.clone())?;
        let mut slot_producers = vec![];
        let mut slot_endorsers = vec![];
        while slot < end {
            if let Some(selection) = cache
                .get(slot.get_cycle(self.periods_per_cycle))
                .and_then(|selections| selections.draws.get(&slot))
            {
                if selection.producer == *address {
                    slot_producers.push(slot);
                } else if let Some(index) = selection.endorsements.iter().position(|e| e == address)
                {
                    slot_endorsers.push(IndexedSlot { slot, index });
                }
            }
            slot = match slot.get_next_slot(self.thread_count) {
                Ok(next_slot) => next_slot,
                _ => break,
            };
        }
        Ok((slot_producers, slot_endorsers))
    }

    /// Returns a boxed clone of self.
    /// Allows cloning `Box<dyn SelectorController>`,
    /// see `massa-pos-exports/controller_traits.rs`
    fn clone_box(&self) -> Box<dyn SelectorController> {
        Box::new(self.clone())
    }

    /// Get every [Selection]
    ///
    /// Only used in tests for post-bootstrap selection matching.
    #[cfg(feature = "testing")]
    fn get_entire_selection(&self) -> VecDeque<(u64, HashMap<Slot, Selection>)> {
        let (_, lock) = &*self.cache;
        let cache_guard = lock.read();
        let cache = cache_guard.as_ref().map_err(|err| err.clone()).unwrap();
        cache
            .0
            .iter()
            .map(|cycle_draws| (cycle_draws.cycle, cycle_draws.draws.clone()))
            .collect()
    }
}

/// Implementation of the Selector manager
/// Allows stopping the selector worker
pub(crate) struct SelectorManagerImpl {
    /// handle used to join the worker thread
    pub(crate) thread_handle: Option<std::thread::JoinHandle<PosResult<()>>>,
    /// Input data mpsc (used to stop the selector thread)
    pub(crate) input_mpsc: SyncSender<Command>,
}

impl SelectorManager for SelectorManagerImpl {
    /// stops the worker
    fn stop(&mut self) {
        info!("stopping selector worker...");
        let _ = self.input_mpsc.send(Command::Stop);
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
