// Copyright (c) 2022 MASSA LABS <info@massa.net>

use std::collections::BTreeMap;
use std::thread::JoinHandle;

use massa_hash::Hash;
use massa_models::prehash::Map;
use massa_models::Address;
use massa_pos_exports::PosResult;
use massa_pos_exports::SelectorConfig;
use massa_pos_exports::SelectorController;
use massa_pos_exports::SelectorManager;

use crate::controller::SelectorControllerImpl;
use crate::controller::SelectorManagerImpl;
use crate::{Command, DrawCachePtr, InputDataPtr};

/// Structure gathering all elements needed by the selector thread
#[allow(dead_code)]
pub(crate) struct SelectorThread {
    // A copy of the input data allowing access to incoming requests
    pub(crate) input_data: InputDataPtr,
    /// Cache of computed endorsements
    pub(crate) cache: DrawCachePtr,
    /// Configuration
    pub(crate) cfg: SelectorConfig,
    /// Initial rolls from initial rolls file.
    /// Used when seeking roll distributions for negative cycle numbers (-3, -2, -1)
    pub(crate) initial_rolls: Map<Address, u64>,
    /// Initial random seeds: they are lightweight, we always keep them
    /// Those seeds are used when seeking the seed for cycles -2 and -1 (in that order)
    pub(crate) initial_seeds: Vec<Hash>,
    /// Computed cycle rolls cumulative distribution to keep in memory,
    /// Map<Cycle, CumulativeDistrib>
    pub(crate) cycle_states: BTreeMap<u64, Vec<(u64, Address)>>,
}

impl SelectorThread {
    /// Creates the `SelectorThread` structure to gather all data and references
    /// needed by the selector worker thread.
    ///
    pub(crate) fn spawn(
        input_data: InputDataPtr,
        cache: DrawCachePtr,
        initial_rolls: Map<Address, u64>,
        cfg: SelectorConfig,
    ) -> JoinHandle<PosResult<()>> {
        std::thread::spawn(|| {
            let this = Self {
                input_data,
                cache,
                initial_seeds: generate_initial_seeds(&cfg),
                cfg,
                cycle_states: Default::default(),
                initial_rolls,
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
            let cycle_info = {
                let mut data = self.input_data.1.lock();
                match data.pop_front() {
                    Some(Command::CycleInfo(cycle_info)) => Some(cycle_info),
                    Some(Command::Stop) => break,
                    None => None,
                }
            };

            if let Some(cycle_info) = cycle_info {
                self.draws(cycle_info)?;
            }

            // Wait to be notified of new input
            // The return value is ignored because we don't care what woke up the condition variable.
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
    let cache = DrawCachePtr::default();
    let controller = SelectorControllerImpl {
        input_data: input_data.clone(),
        cache: cache.clone(),
        periods_per_cycle: selector_config.periods_per_cycle,
        thread_count: selector_config.thread_count,
    };

    // launch the selector thread
    let thread_handle = SelectorThread::spawn(
        input_data.clone(),
        cache,
        get_initial_rolls(&selector_config).unwrap(),
        selector_config,
    );
    let manager = SelectorManagerImpl {
        thread_handle: Some(thread_handle),
        input_data,
    };
    (Box::new(manager), Box::new(controller))
}

/// Generates 3 seeds. The seeds should be used as the initial seeds for negative cycles down to -2
fn generate_initial_seeds(cfg: &SelectorConfig) -> Vec<Hash> {
    let mut cur_seed = Hash::compute_from(cfg.initial_draw_seed.as_bytes());
    vec![cur_seed, Hash::compute_from(cur_seed.to_bytes())]
}

/// Read initial rolls file.
///
/// File path is `cfg.initial_rolls_path`
fn get_initial_rolls(cfg: &SelectorConfig) -> PosResult<Map<Address, u64>> {
    let res = serde_json::from_str::<Map<Address, u64>>(&std::fs::read_to_string(
        &cfg.initial_rolls_path,
    )?)?;
    Ok(res)
}
