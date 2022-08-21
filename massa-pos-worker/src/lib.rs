// Copyright (c) 2022 MASSA LABS <info@massa.net>

// Features used for draining cache in selector thread
#![feature(map_first_last)]

mod controller;
mod draw;
mod worker;

use massa_hash::Hash;
use massa_models::{prehash::Map, Address, Slot};
use massa_pos_exports::{PosResult, Selection};

use parking_lot::{Condvar, Mutex, RwLock};
use std::{
    collections::{HashMap, VecDeque},
    sync::Arc,
};

/// Enumeration of internal commands sent to the selector thread as input
/// datas. `CycleInfo`, Look at [InputDataPtr]
pub(crate) enum Command {
    /// Input requirements for the draw
    DrawInput {
        cycle: u64,
        lookback_rolls: Map<Address, u64>,
        lookback_seed: Hash,
    },
    /// Stop the thread (usually sent by the manager and pushed at the top
    /// of the command queue)
    Stop,
}

/// Same structure pointer that will be used by the selector controller and his
/// thread.
///
/// - `CycleInfo`: stores the new CycleInfo declared by massa (in the
///     Execution module) and will be used to compute the draws in background.
/// - `Stop`: break the thread loop.
pub(crate) type InputDataPtr = Arc<(Condvar, Mutex<VecDeque<Command>>)>;

/// Draw cache (lowest index = oldest)
pub(crate) struct DrawCache(pub VecDeque<CycleDraws>);

impl DrawCache {
    /// get the index of a cycle in the cache
    pub fn get_cycle_index(&self, cycle: u64) -> Option<usize> {
        let first_cycle = match self.0.front() {
            Some(c) => c.cycle,
            None => return None, // history empty
        };
        if cycle < first_cycle {
            return None; // in the past
        }
        let index: usize = match (cycle - first_cycle).try_into() {
            Ok(idx) => idx,
            Err(_) => return None, // usize overflow
        };
        if index >= self.0.len() {
            return None; // in the future
        }
        Some(index)
    }

    /// get a reference to the draws of a given cycle
    pub fn get(&self, cycle: u64) -> Option<&CycleDraws> {
        self.get_cycle_index(cycle).and_then(|idx| self.0.get(idx))
    }
}

/// Draws for a cycle, used in selector cache
pub(crate) struct CycleDraws {
    /// cycle number
    pub cycle: u64,
    /// cache of draws
    pub draws: HashMap<Slot, Selection>,
}

/// Structure of the shared pointer to the computed draws.
pub(crate) type DrawCachePtr = Arc<RwLock<DrawCache>>;

/// Gives the current status (last drawn cycle and error if the thread stopped)
pub(crate) type StatusPtr = Arc<(Condvar, Mutex<PosResult<Option<u64>>>)>;

/// Start thread selector
pub use worker::start_selector_worker;
