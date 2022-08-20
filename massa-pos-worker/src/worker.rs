// Copyright (c) 2022 MASSA LABS <info@massa.net>

use std::collections::BTreeMap;
use std::sync::mpsc;
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
use crate::{Command, DrawCachePtr};

/// Structure gathering all elements needed by the selector thread
#[allow(dead_code)]
pub(crate) struct SelectorThread {
    // A copy of the input data allowing access to incoming requests
    pub(crate) input_mpsc: mpsc::Receiver<Command>,
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
    pub(crate) fn spawn(
        input_mpsc: mpsc::Receiver<Command>,
        cache: DrawCachePtr,
        initial_rolls: Vec<Map<Address, u64>>,
        cfg: SelectorConfig,
    ) -> JoinHandle<PosResult<()>> {
        std::thread::spawn(|| {
            let this = Self {
                input_mpsc,
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
        while let Ok(command) = self.input_mpsc.recv() {
            match command {
                // feed cycle
                Command::CycleInfo(cycle_info) => {
                    self.draws(cycle_info)?;
                }

                // stop
                Command::Stop => return Ok(()),
            }
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
    let cache = DrawCachePtr::default();
    let (input_mpsc_tx, input_mpsc_rx) = mpsc::channel();

    let controller = SelectorControllerImpl {
        input_mpsc: input_mpsc_tx.clone(),
        cache: cache.clone(),
        periods_per_cycle: selector_config.periods_per_cycle,
        thread_count: selector_config.thread_count,
    };

    // launch the selector thread
    let thread_handle = SelectorThread::spawn(
        input_mpsc_rx,
        cache,
        get_initial_rolls(&selector_config).unwrap(),
        selector_config,
    );
    let manager = SelectorManagerImpl {
        thread_handle: Some(thread_handle),
        input_mpsc: input_mpsc_tx,
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
    if rolls_per_cycle.len() < cfg.lookback_cycles as usize {
        return Err(InvalidInitialRolls(
            cfg.lookback_cycles,
            rolls_per_cycle.len(),
        ));
    }
    Ok(rolls_per_cycle)
}
