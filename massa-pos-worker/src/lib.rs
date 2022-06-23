// Copyright (c) 2022 MASSA LABS <info@massa.net>

mod controller;
mod worker;

use massa_pos_exports::CycleInfo;

use parking_lot::{Condvar, Mutex};
use std::{collections::VecDeque, sync::Arc};

/// Same structure pointer that will be used by the selector controller and his
/// thread. It will store all new CycleInfo declared by massa (in the
/// Execution module) and will be used to compute the draws in background.
pub(crate) type InputDataPtr = Arc<(Condvar, Mutex<VecDeque<CycleInfo>>)>;

pub use worker::start_selector_worker;
