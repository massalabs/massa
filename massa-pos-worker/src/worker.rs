// Copyright (c) 2022 MASSA LABS <info@massa.net>

use std::collections::BTreeMap;
use std::sync::{atomic::AtomicBool, Arc};
use std::thread::JoinHandle;

use massa_hash::Hash;
use massa_models::prehash::Map;
use massa_models::Address;
use massa_pos_exports::PosError::InvalidInitialRolls;
use massa_pos_exports::PosResult;
use massa_pos_exports::SelectorConfig;
use massa_pos_exports::SelectorController;
use massa_pos_exports::SelectorManager;

use crate::controller::SelectorControllerImpl;
use crate::controller::SelectorManagerImpl;
use crate::DrawCachePtr;
use crate::InputDataPtr;

/// Structure gathering all elements needed by the selector thread
pub(crate) struct SelectorThread {
    // A copy of the input data allowing access to incoming requests
    pub(crate) input_data: InputDataPtr,
    /// Cache of computed endorsements
    pub(crate) cache: DrawCachePtr,
    /// Configuration
    pub(crate) cfg: SelectorConfig,
    /// Initial rolls from initial rolls file
    pub(crate) initial_rolls: Vec<Map<Address, u64>>,
    /// Initial seeds: they are lightweight, we always keep them
    /// the seed for cycle -N is obtained by hashing N times the value
    /// `ConsensusConfig.initial_draw_seed` the seeds are indexed from -1 to -N
    pub(crate) initial_seeds: Vec<Vec<u8>>,
    /// Computed cycle rolls cumulative distribution to keep in memory,
    /// Map<Cycle, CumulativeDistrib>
    pub(crate) cycle_states: BTreeMap<u64, Vec<(u64, Address)>>,
}

impl SelectorThread {
    /// Creates the `SelectorThread` structure to gather all data and references
    /// needed by the selector worker thread.
    ///
    /// # Arguments
    /// * `input_data`: a copy of the input data interface to get incoming requests from
    pub(crate) fn spawn(
        input_data: InputDataPtr,
        cache: DrawCachePtr,
        initial_rolls: Vec<Map<Address, u64>>,
        stop_flag: Arc<AtomicBool>,
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

            this.run(stop_flag)
        })
    }

    /// Check if cycle info changed or new and compute the draws
    /// for future cycle.
    /// # Arguments
    /// * `cycle_info`: a cycle info with roll counts, seed, etc...
    fn run(mut self, stop_flag: Arc<AtomicBool>) -> PosResult<()> {
        loop {
            if stop_flag.load(std::sync::atomic::Ordering::Relaxed) {
                break;
            }
            let input_data = self.input_data.clone();
            let mut data = input_data.1.lock();
            if let Some(cycle_info) = data.pop_front() {
                self.draws(cycle_info)?
            }
            self.prune_cache();
            // Wait to be notified of new input
            // The return value is ignored because we don't care what woke up the condition variable.
            let _ = self.input_data.0.wait(&mut data);
        }
        Ok(())
    }
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
    selector_config: SelectorConfig,
) -> (Box<dyn SelectorManager>, Box<dyn SelectorController>) {
    let input_data = InputDataPtr::default();
    let cache = DrawCachePtr::default();
    let controller = SelectorControllerImpl {
        input_data: input_data.clone(),
        cache: cache.clone(),
        periods_per_cycle,
    };

    // launch the selector thread
    let input_data_clone = input_data.clone();
    let cache_clone = cache.clone();
    let initial_rolls = get_initial_rolls(&selector_config).unwrap();
    let stop_flag = Arc::new(AtomicBool::new(false));
    let stop_flag_clone = stop_flag.clone();
    let thread_handle = SelectorThread::spawn(
        input_data_clone,
        cache_clone,
        initial_rolls,
        stop_flag_clone,
        selector_config,
    );
    let manager = SelectorManagerImpl {
        thread_handle: Some(thread_handle),
        stop_flag,
    };
    (Box::new(manager), Box::new(controller))
}

/// Generates N seeds. The seeds should be used as the initial seeds between
/// cycle 0 and cycle N.
///
/// N is `cfg.lookback_cycles` and must be >= 2
fn generate_initial_seeds(cfg: &SelectorConfig) -> Vec<Vec<u8>> {
    let mut cur_seed = cfg.initial_draw_seed.as_bytes().to_vec();
    let mut initial_seeds = vec![];
    for _ in 0..=cfg.lookback_cycles {
        cur_seed = Hash::compute_from(&cur_seed).to_bytes().to_vec();
        initial_seeds.push(cur_seed.clone());
    }
    initial_seeds
}

/// Read initial rolls file. Return a vector containing the initial rolls for
/// the cycle < `cfg.loopback_cycle`
///
/// File path is `cfg.initial_rolls_path`
fn get_initial_rolls(cfg: &SelectorConfig) -> PosResult<Vec<Map<Address, u64>>> {
    let rolls_per_cycle = serde_json::from_str::<Vec<Map<Address, u64>>>(
        &std::fs::read_to_string(&cfg.initial_rolls_path)?,
    )?;
    if rolls_per_cycle.len() < cfg.lookback_cycles {
        return Err(InvalidInitialRolls(
            cfg.lookback_cycles,
            rolls_per_cycle.len(),
        ));
    }
    Ok(rolls_per_cycle)
}
