use crate::{block_graph::ActiveBlock, ConsensusConfig, ConsensusError};
use bitvec::prelude::*;
use crypto::hash::Hash;
use models::{Address, BlockId, Operation, Slot};
use rand::distributions::Uniform;
use rand::Rng;
use rand_xoshiro::rand_core::SeedableRng;
use rand_xoshiro::Xoshiro256PlusPlus;
use serde::{Deserialize, Serialize};
use std::collections::{btree_map, hash_map, BTreeMap, HashMap, HashSet, VecDeque};
use std::convert::TryInto;

pub trait RollUpdateInterface {
    fn chain(&mut self, change: &Self) -> Result<(), ConsensusError>;
    fn is_nil(&self) -> bool;
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RollUpdate {
    pub roll_increment: bool, // true = majority buy, false = majority sell
    pub roll_delta: u64,      // absolute change in roll count
                              // Here is space for registering any denunciations/resets
}

impl RollUpdateInterface for RollUpdate {
    fn chain(&mut self, change: &Self) -> Result<(), ConsensusError> {
        if self.roll_increment == change.roll_increment {
            self.roll_delta = self.roll_delta.checked_add(change.roll_delta).ok_or(
                ConsensusError::InvalidRollUpdate("overflow in RollUpdate::chain".into()),
            )?;
        } else if change.roll_delta > self.roll_delta {
            self.roll_delta = change.roll_delta.checked_sub(self.roll_delta).ok_or(
                ConsensusError::InvalidRollUpdate("underflow in RollUpdate::chain".into()),
            )?;
            self.roll_increment = !self.roll_increment;
        } else {
            self.roll_delta = self.roll_delta.checked_sub(change.roll_delta).ok_or(
                ConsensusError::InvalidRollUpdate("underflow in RollUpdate::chain".into()),
            )?;
        }
        if self.roll_delta == 0 {
            self.roll_increment = true;
        }
        Ok(())
    }

    fn is_nil(&self) -> bool {
        self.roll_delta == 0
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RollUpdates(pub HashMap<Address, RollUpdate>);

impl RollUpdates {
    pub fn new() -> Self {
        RollUpdates(HashMap::new())
    }

    pub fn apply(&mut self, addr: &Address, update: &RollUpdate) -> Result<(), ConsensusError> {
        if update.is_nil() {
            return Ok(());
        }
        match self.0.entry(*addr) {
            hash_map::Entry::Occupied(mut occ) => {
                occ.get_mut().chain(update)?;
                if occ.get().is_nil() {
                    occ.remove();
                }
            }
            hash_map::Entry::Vacant(vac) => {
                vac.insert(update.clone());
            }
        }
        Ok(())
    }
}

impl RollUpdateInterface for RollUpdates {
    fn chain(&mut self, change: &Self) -> Result<(), ConsensusError> {
        for (addr, update) in change.0.iter() {
            if update.is_nil() {
                continue;
            }
            match self.0.entry(*addr) {
                hash_map::Entry::Occupied(mut occ) => {
                    occ.get_mut().chain(update)?;
                    if occ.get().is_nil() {
                        occ.remove();
                    }
                }
                hash_map::Entry::Vacant(vac) => {
                    vac.insert(update.clone());
                }
            }
        }
        Ok(())
    }

    fn is_nil(&self) -> bool {
        self.0.iter().all(|(_k, v)| v.is_nil())
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RollCounts(pub BTreeMap<Address, u64>);

impl RollCounts {
    pub fn new() -> Self {
        RollCounts(BTreeMap::new())
    }

    pub fn apply(&mut self, updates: &RollUpdates) -> Result<(), ConsensusError> {
        for (addr, update) in updates.0.iter() {
            match self.0.entry(*addr) {
                btree_map::Entry::Occupied(mut occ) => {
                    if update.roll_increment {
                        occ.get_mut().checked_add(update.roll_delta).ok_or(
                            ConsensusError::InvalidRollUpdate(
                                "overflow while incrementing roll count".into(),
                            ),
                        )?;
                    } else {
                        occ.get_mut().checked_sub(update.roll_delta).ok_or(
                            ConsensusError::InvalidRollUpdate(
                                "underflow while decrementing roll count".into(),
                            ),
                        )?;
                    }
                }
                btree_map::Entry::Vacant(vac) => {
                    if update.roll_increment || update.roll_delta == 0 {
                        vac.insert(update.roll_delta);
                    } else {
                        return Err(ConsensusError::InvalidRollUpdate(
                            "underflow while decrementing roll count".into(),
                        ));
                    }
                }
            }
        }
        Ok(())
    }

    pub fn remove_nil(&mut self) {
        self.0.retain(|_k, v| *v != 0);
    }

    pub fn clone_subset(&self, addrs: &HashSet<Address>) -> Self {
        Self(
            addrs
                .iter()
                .filter_map(|addr| self.0.get(addr).map(|v| (*addr, *v)))
                .collect(),
        )
    }
}

pub trait OperationRollInterface {
    fn get_roll_updates(&self) -> Result<RollUpdates, ConsensusError>;
}

impl OperationRollInterface for Operation {
    fn get_roll_updates(&self) -> Result<RollUpdates, ConsensusError> {
        let mut res = RollUpdates::new();
        match self.content.op {
            models::OperationType::Transaction { .. } => {}
            models::OperationType::RollBuy { roll_count } => {
                res.apply(
                    &Address::from_public_key(&self.content.sender_public_key)?,
                    &RollUpdate {
                        roll_increment: true,
                        roll_delta: roll_count,
                    },
                )?;
            }
            models::OperationType::RollSell { roll_count } => {
                res.apply(
                    &Address::from_public_key(&self.content.sender_public_key)?,
                    &RollUpdate {
                        roll_increment: false,
                        roll_delta: roll_count,
                    },
                )?;
            }
        }
        Ok(res)
    }
}

pub struct ThreadCycleState {
    /// Cycle number
    cycle: u64,
    /// Last final slot (can be a miss)
    last_final_slot: Slot,
    /// Number of rolls an address has
    pub roll_count: RollCounts,
    /// Compensated roll updates
    pub cycle_updates: RollUpdates,
    /// Used to seed the random selector at each cycle
    rng_seed: BitVec,
}

pub struct ProofOfStake {
    /// Config
    cfg: ConsensusConfig,
    /// Index by thread and cycle number
    cycle_states: Vec<VecDeque<ThreadCycleState>>,
    /// Cycle draw cache: cycle_number => (counter, map(slot => address))
    draw_cache: HashMap<u64, (usize, HashMap<Slot, Address>)>,
    draw_cache_counter: usize,
    /// Initial rolls: we keep them as long as negative cycle draws are needed
    initial_rolls: Option<Vec<RollCounts>>,
    // Initial seeds: they are lightweight, we always keep them
    // the seed for cycle -N is obtained by hashing N times the value ConsensusConfig.initial_draw_seed
    // the seeds are indexed from -1 to -N
    initial_seeds: Vec<Vec<u8>>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ExportProofOfStake {
    /// Index by thread and cycle number
    cycle_states: Vec<VecDeque<ExportThreadCycleState>>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct ExportThreadCycleState {
    /// Cycle number
    cycle: u64,
    /// Last final slot (can be a miss)
    last_final_slot: Slot,
    /// number of rolls an address has
    roll_count: Vec<(Address, u64)>,
    /// Compensated roll updates
    cycle_updates: Vec<(Address, RollUpdate)>,
    /// Used to seed random selector at each cycle
    rng_seed: BitVec,
}

impl ProofOfStake {
    pub async fn new(
        cfg: ConsensusConfig,
        genesis_block_ids: &Vec<BlockId>,
    ) -> Result<ProofOfStake, ConsensusError> {
        let mut cycle_states = Vec::with_capacity(cfg.thread_count as usize);
        let initial_roll_count = ProofOfStake::get_initial_rolls(&cfg).await?;
        for (thread, thread_rolls) in initial_roll_count.iter().enumerate() {
            // init thread history with one cycle
            let mut rng_seed = BitVec::new();
            rng_seed.push(genesis_block_ids[thread].get_first_bit());
            let mut history = VecDeque::with_capacity(
                (cfg.pos_lock_cycles + cfg.pos_lock_cycles + 2 + 1) as usize,
            );
            let thread_cycle_state = ThreadCycleState {
                cycle: 0,
                last_final_slot: Slot::new(0, thread as u8),
                roll_count: thread_rolls.clone(),
                cycle_updates: RollUpdates::new(),
                rng_seed,
            };
            history.push_front(thread_cycle_state);
            cycle_states.push(history);
        }
        // generate object
        Ok(ProofOfStake {
            cycle_states,
            initial_rolls: Some(initial_roll_count),
            initial_seeds: ProofOfStake::generate_initial_seeds(&cfg),
            draw_cache: HashMap::with_capacity(cfg.pos_draw_cached_cycles),
            cfg,
            draw_cache_counter: 0,
        })
    }

    async fn get_initial_rolls(cfg: &ConsensusConfig) -> Result<Vec<RollCounts>, ConsensusError> {
        Ok(serde_json::from_str::<Vec<BTreeMap<Address, u64>>>(
            &tokio::fs::read_to_string(&cfg.initial_rolls_path).await?,
        )?
        .into_iter()
        .map(|itm| RollCounts(itm))
        .collect())
    }

    fn generate_initial_seeds(cfg: &ConsensusConfig) -> Vec<Vec<u8>> {
        let mut cur_seed = cfg.initial_draw_seed.as_bytes().to_vec();
        let mut initial_seeds =
            Vec::with_capacity((cfg.pos_lock_cycles + cfg.pos_lock_cycles + 1) as usize);
        for _ in 0..(cfg.pos_lock_cycles + cfg.pos_lock_cycles + 1) {
            cur_seed = Hash::hash(&cur_seed).to_bytes().to_vec();
            initial_seeds.push(cur_seed.clone());
        }
        initial_seeds
    }

    pub fn export(&self) -> ExportProofOfStake {
        ExportProofOfStake {
            cycle_states: self
                .cycle_states
                .iter()
                .map(|vec| {
                    vec.iter()
                        .map(|frtd| frtd.export())
                        .collect::<VecDeque<ExportThreadCycleState>>()
                })
                .collect(),
        }
    }

    pub async fn from_export(
        cfg: ConsensusConfig,
        export: ExportProofOfStake,
    ) -> Result<ProofOfStake, ConsensusError> {
        let cycle_states: Vec<VecDeque<ThreadCycleState>> = export
            .cycle_states
            .into_iter()
            .map(|vec| {
                vec.into_iter()
                    .map(|frtd| ThreadCycleState::from_export(frtd))
                    .collect::<VecDeque<ThreadCycleState>>()
            })
            .collect();

        // if initial rolls are still needed for some threads, load them
        let initial_rolls = if cycle_states
            .iter()
            .any(|v| v[0].cycle < cfg.pos_lock_cycles + cfg.pos_lock_cycles + 1)
        {
            Some(ProofOfStake::get_initial_rolls(&cfg).await?)
        } else {
            None
        };

        // generate object
        Ok(ProofOfStake {
            cycle_states,
            initial_rolls,
            initial_seeds: ProofOfStake::generate_initial_seeds(&cfg),
            draw_cache: HashMap::with_capacity(cfg.pos_draw_cached_cycles),
            cfg,
            draw_cache_counter: 0,
        })
    }

    fn get_cycle_draws(&mut self, cycle: u64) -> Result<&HashMap<Slot, Address>, ConsensusError> {
        self.draw_cache_counter += 1;

        // check if cycle is already in cache
        if let Some((r_cnt, _r_map)) = self.draw_cache.get_mut(&cycle) {
            // increment counter
            *r_cnt = self.draw_cache_counter;
            return Ok(&self.draw_cache[&cycle].1);
        }

        // truncate cache to keep only the desired number of elements
        // we do it first to free memory space
        while self.draw_cache.len() >= self.cfg.pos_draw_cached_cycles {
            if let Some(slot_to_remove) = self
                .draw_cache
                .iter()
                .min_by_key(|(_slot, (c_cnt, _map))| c_cnt)
                .map(|(slt, _)| *slt)
            {
                self.draw_cache.remove(&slot_to_remove.clone());
            } else {
                break;
            }
        }

        // get rolls and seed
        let blocks_in_cycle = self.cfg.periods_per_cycle as usize * self.cfg.thread_count as usize;
        let (cum_sum, rng_seed) = if cycle >= self.cfg.pos_lookback_cycles + 1 {
            // nominal case: lookback after or at cycle 0
            let target_cycle = cycle - self.cfg.pos_lookback_cycles - 1;

            // get final data for all threads
            let mut rng_seed_bits = BitVec::<Lsb0, u8>::with_capacity(blocks_in_cycle);
            let target_cycle_last_period = (target_cycle + 1) * self.cfg.periods_per_cycle - 1;
            let mut cum_sum: Vec<(u64, Address)> = Vec::new(); // amount, thread, address
            let mut cum_sum_cursor = 0u64;
            for scan_thread in 0..self.cfg.thread_count {
                let final_data = self.get_final_roll_data(target_cycle, scan_thread).ok_or(
                    ConsensusError::PosCycleUnavailable(format!(
                    "trying to get PoS draw rolls/seed for cycle {} thread {} which is unavailable",
                    target_cycle, scan_thread
                )),
                )?;
                if final_data.last_final_slot.period != target_cycle_last_period {
                    // the target cycle is not final yet
                    return Err(ConsensusError::PosCycleUnavailable(format!("tryign to get PoS draw rolls/seed for cycle {} thread {} which is not finalized yet", target_cycle, scan_thread)));
                }
                rng_seed_bits.extend(&final_data.rng_seed);
                for (addr, &n_rolls) in final_data.roll_count.0.iter() {
                    if n_rolls == 0 {
                        continue;
                    }
                    cum_sum_cursor += n_rolls;
                    cum_sum.push((cum_sum_cursor, addr.clone()));
                }
            }
            // compute the RNG seed from the seed bits
            let rng_seed = Hash::hash(&rng_seed_bits.into_vec()).to_bytes().to_vec();

            (cum_sum, rng_seed)
        } else {
            // special case: lookback before cycle 0

            // get initial rolls
            let mut cum_sum: Vec<(u64, Address)> = Vec::new(); // amount, thread, address
            let mut cum_sum_cursor = 0u64;
            for scan_thread in 0..self.cfg.thread_count {
                let init_rolls = &self.initial_rolls.as_ref().ok_or(
                    ConsensusError::PosCycleUnavailable(format!(
                    "trying to get PoS initial draw rolls/seed for negative cycle at thread {}, which is unavailable",
                    scan_thread
                )))?[scan_thread as usize];
                for (addr, &n_rolls) in init_rolls.0.iter() {
                    if n_rolls == 0 {
                        continue;
                    }
                    cum_sum_cursor += n_rolls;
                    cum_sum.push((cum_sum_cursor, addr.clone()));
                }
            }

            // get RNG seed
            let seed_idx = self.cfg.pos_lookback_cycles - cycle;
            let rng_seed = self.initial_seeds[seed_idx as usize].clone();

            (cum_sum, rng_seed)
        };
        let cum_sum_max = cum_sum
            .last()
            .ok_or(ConsensusError::ContainerInconsistency(
                "draw cum_sum is empty".into(),
            ))?
            .0;

        // init RNG
        let mut rng = Xoshiro256PlusPlus::from_seed(rng_seed.try_into().map_err(|_| {
            ConsensusError::ContainerInconsistency("could not seed RNG with computed seed".into())
        })?);

        // perform draws
        let distribution = Uniform::new(0, cum_sum_max);
        let mut draws: HashMap<Slot, Address> = HashMap::with_capacity(blocks_in_cycle);
        let cycle_first_period = cycle * self.cfg.periods_per_cycle;
        let cycle_last_period = (cycle + 1) * self.cfg.periods_per_cycle - 1;
        if cycle_first_period == 0 {
            // genesis slots: force creator address draw
            let genesis_addr = Address::from_public_key(&crypto::signature::derive_public_key(
                &self.cfg.genesis_key,
            ))?;
            for draw_thread in 0..self.cfg.thread_count {
                draws.insert(Slot::new(0, draw_thread), genesis_addr);
            }
        }
        for draw_period in cycle_first_period..=cycle_last_period {
            if draw_period == 0 {
                // do not draw genesis again
                continue;
            }
            for draw_thread in 0..self.cfg.thread_count {
                let sample = rng.sample(&distribution);

                // locate the draw in the cum_sum through binary search
                let found_index = match cum_sum.binary_search_by_key(&sample, |(c_sum, _)| *c_sum) {
                    Ok(idx) => idx + 1,
                    Err(idx) => idx,
                };
                let (_sum, found_addr) = cum_sum[found_index];

                draws.insert(Slot::new(draw_period, draw_thread), found_addr);
            }
        }

        // add new cache element
        Ok(&self
            .draw_cache
            .entry(cycle)
            .or_insert((self.draw_cache_counter, draws))
            .1)
    }

    pub fn draw(&mut self, slot: Slot) -> Result<Address, ConsensusError> {
        let cycle = slot.get_cycle(self.cfg.periods_per_cycle);
        let cycle_draws = self.get_cycle_draws(cycle)?;
        Ok(cycle_draws
            .get(&slot)
            .ok_or(ConsensusError::ContainerInconsistency(format!(
                "draw cycle computed for cycle {} but slot {} absent",
                cycle, slot
            )))?
            .clone())
    }

    /// Update internal states after a set of blocks become final
    /// see /consensus/pos.md#when-a-block-b-in-thread-tau-and-cycle-n-becomes-final
    pub fn note_final_blocks(
        &mut self,
        blocks: &HashMap<BlockId, ActiveBlock>,
    ) -> Result<(), ConsensusError> {
        // Update internal states after a set of blocks become final.

        // process blocks by increasing slot number
        let mut indices: Vec<(Slot, BlockId)> = blocks
            .iter()
            .map(|(k, v)| (v.block.header.content.slot, *k))
            .collect();
        indices.sort_unstable();
        for (block_slot, block_id) in indices.into_iter() {
            let a_block = &blocks[&block_id];
            let thread = block_slot.thread;

            // for this thread, iterate from the latest final period + 1 to the block's
            // all iterations for which period < block_slot.period are misses
            // the iteration at period = block_slot.period corresponds to a_block
            let cur_last_final_period =
                self.cycle_states[thread as usize][0].last_final_slot.period;
            for period in (cur_last_final_period + 1)..=block_slot.period {
                let cycle = period / self.cfg.periods_per_cycle;
                let slot = Slot::new(period, thread);

                // if the cycle of the miss/block being processed is higher than the latest final block cycle
                // then create a new ThreadCycleState representing this new cycle and push it at the front of cycle_states[thread]
                // (step 1 in the spec)
                if cycle
                    > self.cycle_states[thread as usize][0]
                        .last_final_slot
                        .get_cycle(self.cfg.periods_per_cycle)
                {
                    // the new ThreadCycleState inherits from the roll_count of the previous cycle but has empty cycle_purchases, cycle_sales, rng_seed
                    let roll_count = self.cycle_states[thread as usize][0].roll_count.clone();
                    self.cycle_states[thread as usize].push_front(ThreadCycleState {
                        cycle,
                        last_final_slot: slot.clone(),
                        cycle_updates: RollUpdates::new(),
                        roll_count,
                        rng_seed: BitVec::new(),
                    });
                    // If cycle_states becomes longer than pos_lookback_cycles+pos_lock_cycles+1, truncate it by removing the back elements
                    self.cycle_states[thread as usize].truncate(
                        (self.cfg.pos_lookback_cycles + self.cfg.pos_lock_cycles + 2) as usize,
                    );
                }

                // apply the miss/block to the latest cycle_states
                // (step 2 in the spec)
                let entry = &mut self.cycle_states[thread as usize][0];
                // update the last_final_slot for the latest cycle
                entry.last_final_slot = slot.clone();
                // check if we are applying the block itself or a miss
                if period == block_slot.period {
                    // we are applying the block itself
                    entry.cycle_updates.chain(&a_block.roll_updates)?;
                    entry.roll_count.apply(&a_block.roll_updates)?;
                    entry.roll_count.remove_nil();
                    // append the 1st bit of the block's hash to the RNG seed bitfield
                    entry.rng_seed.push(block_id.get_first_bit());
                } else {
                    // we are applying a miss
                    // append the 1st bit of the hash of the slot of the miss to the RNG seed bitfield
                    entry.rng_seed.push(slot.get_first_bit());
                }
            }
        }

        // if initial rolls are not needed, remove them to free memory
        if self.initial_rolls.is_some() {
            if !self
                .cycle_states
                .iter()
                .any(|v| v[0].cycle < self.cfg.pos_lock_cycles + self.cfg.pos_lock_cycles + 1)
            {
                self.initial_rolls = None;
            }
        }

        Ok(())
    }

    pub fn get_last_final_block_cycle(&self, thread: u8) -> u64 {
        self.cycle_states[thread as usize][0].cycle
    }

    pub fn get_final_roll_data(&self, cycle: u64, thread: u8) -> Option<&ThreadCycleState> {
        let last_final_block_cycle = self.get_last_final_block_cycle(thread);
        if let Some(neg_relative_cycle) = last_final_block_cycle.checked_sub(cycle) {
            self.cycle_states[thread as usize].get(neg_relative_cycle as usize)
        } else {
            None
        }
    }
}

impl ThreadCycleState {
    fn export(&self) -> ExportThreadCycleState {
        ExportThreadCycleState {
            cycle: self.cycle,
            last_final_slot: self.last_final_slot,
            roll_count: self.roll_count.0.clone().into_iter().collect(),
            cycle_updates: self.cycle_updates.0.clone().into_iter().collect(),
            rng_seed: self.rng_seed.clone(),
        }
    }

    fn from_export(export: ExportThreadCycleState) -> ThreadCycleState {
        ThreadCycleState {
            cycle: export.cycle,
            last_final_slot: export.last_final_slot,
            roll_count: RollCounts(export.roll_count.into_iter().collect()),
            cycle_updates: RollUpdates(export.cycle_updates.into_iter().collect()),
            rng_seed: export.rng_seed,
        }
    }
}
