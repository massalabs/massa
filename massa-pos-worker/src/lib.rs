// Copyright (c) 2022 MASSA LABS <info@massa.net>

// Features used for draining cache in selector thread
#![feature(btree_drain_filter)]
#![feature(map_first_last)]

mod controller;
mod draw;
mod worker;

use massa_models::Slot;
use massa_pos_exports::{CycleInfo, Selection};

use parking_lot::{Condvar, Mutex, RwLock};
use std::{
    collections::{BTreeMap, HashMap, VecDeque},
    sync::Arc,
};

/// Same structure pointer that will be used by the selector controller and his
/// thread. It will store all new CycleInfo declared by massa (in the
/// Execution module) and will be used to compute the draws in background.
pub(crate) type InputDataPtr = Arc<(Condvar, Mutex<VecDeque<CycleInfo>>)>;

/// Structure of the shared pointer to the computed draws.
pub(crate) type DrawCachePtr = Arc<RwLock<BTreeMap<u64, HashMap<Slot, Selection>>>>;

/// Start thread selector
pub use worker::start_selector_worker;
