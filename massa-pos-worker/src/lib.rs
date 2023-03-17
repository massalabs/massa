// Copyright (c) 2022 MASSA LABS <info@massa.net>

// Features used for draining cache in selector thread

mod controller;
mod draw;
mod worker;

use massa_hash::Hash;
use massa_models::{address::Address, slot::Slot};
use massa_pos_exports::{PosResult, Selection};

use parking_lot::{Condvar, Mutex, RwLock, RwLockReadGuard};
use std::{
    collections::{BTreeMap, HashMap, VecDeque},
    sync::Arc,
};

/// Enumeration of internal commands sent to the selector thread as input
/// data. `CycleInfo`, Look at `InputDataPtr`
pub(crate) enum Command {
    /// Input requirements for the draw
    DrawInput {
        cycle: u64,
        lookback_rolls: BTreeMap<Address, u64>,
        lookback_seed: Hash,
        last_start_period: u64
    },
    /// Stop the thread (usually sent by the manager and pushed at the top
    /// of the command queue)
    Stop,
}

/// Draw cache (lowest index = oldest)
#[derive(Debug)]
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
#[derive(Debug)]
pub(crate) struct CycleDraws {
    /// cycle number
    pub cycle: u64,
    /// cache of draws
    pub draws: HashMap<Slot, Selection>,
}

/// Structure of the shared pointer to the computed draws, or error if the draw system failed.
pub(crate) type DrawCachePtr = Arc<(RwLockCondvar, RwLock<PosResult<DrawCache>>)>;

/// Start thread selector
pub use worker::start_selector_worker;

// an RwLock condvar
#[derive(Default)]
struct RwLockCondvar {
    mutex: Mutex<()>,
    condvar: Condvar,
}

impl RwLockCondvar {
    fn wait<T>(&self, rwlock_read_guard: &mut RwLockReadGuard<T>) {
        let mutex_guard = self.mutex.lock();

        RwLockReadGuard::unlocked(rwlock_read_guard, || {
            let mut mutex_guard = mutex_guard;
            self.condvar.wait(&mut mutex_guard);
        });
    }

    fn notify_all(&self) {
        self.condvar.notify_all();
    }
}
