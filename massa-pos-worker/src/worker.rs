// Copyright (c) 2022 MASSA LABS <info@massa.net>

use crate::controller::SelectorControllerImpl;
use crate::controller::SelectorManagerImpl;
use crate::CycleDraws;
use crate::DrawCache;
use crate::RwLockCondvar;
use crate::{Command, DrawCachePtr, InputDataPtr};
use massa_pos_exports::PosError;
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

    /// process the result of a draw
    fn process_draws_result(
        &self,
        cycle: u64,
        draws_result: PosResult<CycleDraws>,
    ) -> PosResult<()> {
        // write-lock the cache
        let (cache_cv, cache_lock) = &*self.cache;
        let mut cache_guard = cache_lock.write();

        // check cache validity and continuity
        let cache = cache_guard.as_mut().map_err(|err| err.clone())?;
        if let Some(last_cycle) = cache.0.back() {
            if last_cycle.cycle.checked_add(1) != Some(cycle) {
                return Err(PosError::ContainerInconsistency(
                    "discontinuity in cycle draws history".into(),
                ));
            }
        }

        // add draw results to cache, or extract error
        let out_result = match draws_result {
            Ok(cycle_draws) => {
                // add to draws
                cache.0.push_back(cycle_draws);

                // truncate cache to keep only the desired number of elements
                while cache.0.len() > self.cfg.max_draw_cache {
                    cache.0.pop_front();
                }

                // no error
                Ok(())
            }
            // draw error
            Err(err) => Err(err),
        };

        // drop the writable reference to the cache
        std::mem::drop(cache);

        // if there was an error, save a clone of the error to the cache
        if let Err(err) = &out_result {
            *cache_guard = Err(err.clone());
        }

        // notify all waiters
        cache_cv.notify_all();

        out_result
    }

    /// Thread loop.
    ///
    /// While a `Stop` command isn't sent, pop `input_data` and compute
    /// draws for future cycle.
    fn run(self) -> PosResult<()> {
        loop {
            // get input command
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

            // add result to cache and notify waiters
            self.process_draws_result(cycle, draws_result)?;

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
    let cache = Arc::new((
        RwLockCondvar::default(),
        RwLock::new(Ok(DrawCache(VecDeque::with_capacity(
            selector_config.max_draw_cache.saturating_add(1),
        )))),
    ));
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
