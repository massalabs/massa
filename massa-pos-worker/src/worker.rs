// Copyright (c) 2022 MASSA LABS <info@massa.net>

use crate::controller::SelectorControllerImpl;
use crate::controller::SelectorManagerImpl;
use crate::DrawCache;
use crate::StatusPtr;
use crate::{Command, DrawCachePtr, InputDataPtr};
use massa_pos_exports::PosResult;
use massa_pos_exports::SelectorConfig;
use massa_pos_exports::SelectorController;
use massa_pos_exports::SelectorManager;
use parking_lot::Condvar;
use parking_lot::Mutex;
use parking_lot::RwLock;
use std::collections::VecDeque;
use std::sync::Arc;
use std::thread::JoinHandle;

/// Structure gathering all elements needed by the selector thread
#[allow(dead_code)]
pub(crate) struct SelectorThread {
    // A copy of the input data allowing access to incoming requests
    pub(crate) input_data: InputDataPtr,
    /// An output state that can be monitored to monitor the last produced cycle, or any errors
    pub(crate) status: StatusPtr,
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
        status: StatusPtr,
        cache: DrawCachePtr,
        cfg: SelectorConfig,
    ) -> JoinHandle<PosResult<()>> {
        std::thread::spawn(|| {
            let this = Self {
                input_data,
                status,
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
                let (cvar, lock) = &*self.input_data;
                let mut data = lock.lock();
                match data.pop_front() {
                    Some(Command::DrawInput {
                        cycle,
                        lookback_rolls,
                        lookback_seed,
                    }) => (cycle, lookback_rolls, lookback_seed),
                    Some(Command::Stop) => break,
                    None => {
                        // spurious wakeup: wait to be notified of new input
                        cvar.wait(&mut data);
                        continue;
                    }
                }
            };

            // perform draws
            let draws_result = self.perform_draws(cycle, lookback_rolls, lookback_seed);

            // notify status
            {
                let (cvar, lock) = &*self.status;
                let mut status = lock.lock();
                match draws_result {
                    Ok(()) => {
                        // signal successful draw
                        *status = Ok(Some(cycle));
                        cvar.notify_all();
                    }
                    Err(err) => {
                        // signal error while performing draws and quit thread
                        *status = Err(err.clone());
                        cvar.notify_all();
                        return Err(err);
                    }
                }
            }

            // Wait to be notified of new input
            let (cvar, lock) = &*self.input_data;
            cvar.wait(&mut lock.lock());
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
    let status = Arc::new((Condvar::new(), Mutex::new(Ok(None))));
    let cache = Arc::new(RwLock::new(DrawCache(VecDeque::with_capacity(
        selector_config.max_draw_cache.saturating_add(1),
    ))));
    let controller = SelectorControllerImpl {
        input_data: input_data.clone(),
        cache: cache.clone(),
        periods_per_cycle: selector_config.periods_per_cycle,
        thread_count: selector_config.thread_count,
        status: status.clone(),
    };

    // launch the selector thread
    let thread_handle = SelectorThread::spawn(input_data.clone(), status, cache, selector_config);

    let manager = SelectorManagerImpl {
        thread_handle: Some(thread_handle),
        input_data,
    };
    (Box::new(manager), Box::new(controller))
}
