// Copyright (c) 2022 MASSA LABS <info@massa.net>

use crate::controller::SelectorControllerImpl;
use crate::controller::SelectorManagerImpl;
use crate::DrawCache;
use crate::{Command, DrawCachePtr, InputDataPtr};
use massa_pos_exports::PosResult;
use massa_pos_exports::SelectorConfig;
use massa_pos_exports::SelectorController;
use massa_pos_exports::SelectorManager;
use parking_lot::RwLock;
use std::collections::VecDeque;
use std::sync::Arc;
use std::thread::JoinHandle;

/// Structure gathering all elements needed by the selector thread
#[allow(dead_code)]
pub(crate) struct SelectorThread {
    // A copy of the input data allowing access to incoming requests
    pub(crate) input_data: InputDataPtr,
    /// Cache of computed endorsements
    pub(crate) cache: DrawCachePtr,
    /// Configuration
    pub(crate) cfg: SelectorConfig,
}

impl SelectorThread {
    /// Creates the `SelectorThread` structure to gather all data and references
    /// needed by the selector worker thread.
    pub(crate) fn spawn(
        input_data: InputDataPtr,
        cache: DrawCachePtr,
        cfg: SelectorConfig,
    ) -> JoinHandle<PosResult<()>> {
        std::thread::spawn(|| {
            let this = Self {
                input_data,
                cache,
                cfg,
            };
            this.run()
        })
    }

    /// Thread loop.
    ///
    /// While a `Stop` command isn't sent, pop `input_data` and compute
    /// draws for future cycle.
    fn run(mut self) -> PosResult<()> {
        loop {
            let (cycle, lookback_rolls, lookback_seed) = {
                let mut data = self.input_data.1.lock();
                match data.pop_front() {
                    Some(Command::DrawInput {
                        cycle,
                        lookback_rolls,
                        lookback_seed,
                    }) => (cycle, lookback_rolls, lookback_seed),
                    Some(Command::Stop) => break,
                    None => {
                        // spurious wakeup: wait to be notified of new input
                        self.input_data.0.wait(&mut data);
                        continue;
                    }
                }
            };

            self.perform_draws(cycle, lookback_rolls, lookback_seed)?; // TODO on draw error, signal upstream that we need a re-bootstrap

            // Wait to be notified of new input
            self.input_data.0.wait(&mut self.input_data.1.lock());
        }
        Ok(())
    }
}

/// Launches a selector worker thread and returns a pair to interact with it.
///
/// # parameters
/// * none
///
/// # Returns
/// A pair `(selector_manager, selector_controller)` where:
/// * `selector_manager`: allows to stop the worker
/// * `selector_controller`: allows sending requests and notifications to the worker
pub fn start_selector_worker(
    selector_config: SelectorConfig,
) -> (Box<dyn SelectorManager>, Box<dyn SelectorController>) {
    let input_data = InputDataPtr::default();
    let cache = Arc::new(RwLock::new(DrawCache(VecDeque::with_capacity(
        selector_config.max_draw_cache.saturating_add(1),
    ))));
    let controller = SelectorControllerImpl {
        input_data: input_data.clone(),
        cache: cache.clone(),
        periods_per_cycle: selector_config.periods_per_cycle,
        thread_count: selector_config.thread_count,
    };

    // launch the selector thread
    let thread_handle = SelectorThread::spawn(input_data.clone(), cache, selector_config);
    let manager = SelectorManagerImpl {
        thread_handle: Some(thread_handle),
        input_data,
    };
    (Box::new(manager), Box::new(controller))
}
