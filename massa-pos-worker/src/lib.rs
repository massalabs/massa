// Copyright (c) 2022 MASSA LABS <info@massa.net>

// Features used for draining cache in selector thread
#![feature(map_first_last)]

mod controller;
mod draw;
mod worker;

use massa_models::Slot;
use massa_pos_exports::{CycleInfo, Selection};

use parking_lot::RwLock;
use std::{
    collections::{BTreeMap, HashMap},
    sync::Arc,
};

/// Enumeration of internal commands sent to the selector thread as input
/// datas. `CycleInfo`, Look at [InputDataPtr]
pub(crate) enum Command {
    /// CycleInfo inserted in the queue
    CycleInfo(CycleInfo),
    /// Stop the thread (usually sent by the manager and pushed at the top
    /// of the command queue)
    Stop,
}

/// Structure of the shared pointer to the computed draws.
pub(crate) type DrawCachePtr = Arc<RwLock<BTreeMap<u64, HashMap<Slot, Selection>>>>;

/// Start thread selector
pub use worker::start_selector_worker;
