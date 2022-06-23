// Copyright (c) 2022 MASSA LABS <info@massa.net>

use massa_pos_exports::CycleInfo;
use massa_pos_exports::SelectorController;
use massa_pos_exports::SelectorManager;

use crate::controller::SelectorControllerImpl;
use crate::controller::SelectorManagerImpl;
use crate::InputDataPtr;

use parking_lot::Condvar;

use std::sync::Arc;

/// Structure gathering all elements needed by the selector thread
pub(crate) struct SelectorThread {
    // A copy of the input data allowing access to incoming requests
    input_data: InputDataPtr,
}

impl SelectorThread {
    /// Creates the `SelectorThread` structure to gather all data and references
    /// needed by the selector worker thread.
    ///
    /// # Arguments
    /// * `input_data`: a copy of the input data interface to get incoming requests from
    pub fn new(input_data: InputDataPtr) -> Self {
        Self { input_data }
    }

    /// Check if cycle info changed or new and compute the draws
    /// for future cycle.
    /// # Arguments
    /// * `cycle_info`: a cycle info with roll counts, seed, etc...
    fn update_cycle(&mut self, _cycle_info: CycleInfo) {}
}

/// Launches an selector worker thread and returns a pair to interact with it.
///
/// # parameters
/// * none
///
/// # Returns
/// A pair `(selector_manager, selector_controller)` where:
/// * `selector_manager`: allows to stop the worker
/// * `selector_controller`: allows sending requests and notifications to the worker
pub fn start_selector_worker(
    periods_per_cycle: u64,
) -> (Box<dyn SelectorManager>, Box<dyn SelectorController>) {
    // define the input data interface
    let input_data = Arc::new((Condvar::new(), Default::default()));

    // create a controller
    let controller = SelectorControllerImpl {
        input_data: input_data.clone(),
        cache: Default::default(),
        periods_per_cycle,
    };

    // launch the selector thread
    let input_data_clone = input_data.clone();
    let thread_handle = std::thread::spawn(|| {
        SelectorThread::new(input_data_clone);
    });

    // create a manager
    let manager = SelectorManagerImpl {
        thread_handle: Some(thread_handle),
    };

    // return the selector manager and controller pair
    (Box::new(manager), Box::new(controller))
}
