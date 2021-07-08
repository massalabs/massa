use crate::{block_graph::ActiveBlock, ConsensusConfig, ConsensusError};
use bitvec::prelude::*;
use crypto::hash::Hash;
use models::{Address, Block, BlockId, Operation, Slot};
use rand::distributions::Uniform;
use rand::Rng;
use rand_xoshiro::rand_core::SeedableRng;
use rand_xoshiro::Xoshiro256PlusPlus;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap, VecDeque};
use std::convert::TryInto;

pub trait OperationPosInterface {
    /// returns [thread][roll_involved_addr](compensated_bought_rolls, compensated_sold_rolls)
    fn get_roll_changes(&self) -> Result<HashMap<Address, (u64, u64)>, ConsensusError>;
}

impl OperationPosInterface for Operation {
    /// returns [thread][roll_involved_addr](compensated_bought_rolls, compensated_sold_rolls)
    fn get_roll_changes(&self) -> Result<HashMap<Address, (u64, u64)>, ConsensusError> {
        let mut res = HashMap::new();
        match self.content.op {
            models::OperationType::Transaction { .. } => {}
            models::OperationType::RollBuy { roll_count } => {
                res.insert(
                    Address::from_public_key(&self.content.sender_public_key)?,
                    (roll_count, 0),
                );
            }
            models::OperationType::RollSell { roll_count } => {
                res.insert(
                    Address::from_public_key(&self.content.sender_public_key)?,
                    (0, roll_count),
                );
            }
        }
        Ok(res)
    }
}

impl OperationPosInterface for Block {
    fn get_roll_changes(&self) -> Result<HashMap<Address, (u64, u64)>, ConsensusError> {
        let mut res = HashMap::new();
        for op in self.operations.iter() {
            let op_res = op.get_roll_changes()?;
            for (address, (bought, sold)) in op_res.into_iter() {
                if let Some(&(old_bought, old_sold)) = res.get(&address) {
                    res.insert(address, (old_bought + bought, old_sold + sold));
                } else {
                    res.insert(address, (bought, sold));
                }
            }
        }
        Ok(res)
    }
}

pub struct FinalRollThreadData {
    /// Cycle number
    cycle: u64,
    /// Slot of the latest final block
    last_final_slot: Slot,
    /// number of rolls an address has
    roll_count: BTreeMap<Address, u64>,
    /// compensated number of rolls an address has bought in the cycle
    cycle_purchases: HashMap<Address, u64>,
    /// compensated number of rolls an address has sold in the cycle
    cycle_sales: HashMap<Address, u64>,
    /// https://docs.rs/bitvec/0.22.3/bitvec/
    /// Used to seed random selector at each cycle
    rng_seed: BitVec,
}

pub struct ProofOfStake {
    /// Config
    cfg: ConsensusConfig,
    /// Index by thread and cycle number
    final_roll_data: Vec<VecDeque<FinalRollThreadData>>,
    /// Cycle draw cache: cycle_number => (counter, map(slot => address))
    draw_cache: HashMap<u64, (usize, HashMap<Slot, Address>)>,
    draw_cache_counter: usize,
    /// Initial rolls: we keep them as long as negative cycle draws are needed
    initial_rolls: Option<Vec<BTreeMap<Address, u64>>>,
    // Initial seeds: they are lightweight, we always keep them
    // the seed for cycle -N is obtained by hashing N times the value ConsensusConfig.initial_draw_seed
    // the seeds are indexed from -1 to -N
    initial_seeds: Vec<Vec<u8>>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ExportProofOfStake {
    /// Index by thread and cycle number
    final_roll_data: Vec<Vec<ExportFinalRollThreadData>>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct ExportFinalRollThreadData {
    /// Cycle number
    cycle: u64,
    last_final_slot: Slot,
    /// number of rolls an address has
    roll_count: Vec<(Address, u64)>,
    /// compensated number of rolls an address has bought in the cycle
    cycle_purchases: Vec<(Address, u64)>,
    /// compensated number of rolls an address has sold in the cycle
    cycle_sales: Vec<(Address, u64)>,
    /// https://docs.rs/bitvec/0.22.3/bitvec/
    /// Used to seed random selector at each cycle
    rng_seed: BitVec,
}

impl ProofOfStake {
    pub async fn new(cfg: ConsensusConfig) -> Result<ProofOfStake, ConsensusError> {
        let mut final_roll_data = Vec::with_capacity(cfg.thread_count as usize);
        let initial_roll_count = ProofOfStake::get_initial_rolls(&cfg).await?;
        for (thread, thread_rolls) in initial_roll_count.iter().enumerate() {
            // init thread history with one cycle
            // note that the genesis blocks are not taken into account in the rng_seed
            let mut history = VecDeque::with_capacity(
                (cfg.pos_lock_cycles + cfg.pos_lock_cycles + 2 + 1) as usize,
            );
            let frtd = FinalRollThreadData {
                cycle: 0,
                last_final_slot: Slot::new(0, thread as u8),
                roll_count: thread_rolls.clone(),
                cycle_purchases: HashMap::new(),
                cycle_sales: HashMap::new(),
                rng_seed: BitVec::new(),
            };
            history.push_front(frtd);
            final_roll_data.push(history);
        }
        // generate object
        Ok(ProofOfStake {
            final_roll_data,
            initial_rolls: Some(initial_roll_count),
            initial_seeds: ProofOfStake::generate_initial_seeds(&cfg),
            draw_cache: HashMap::with_capacity(cfg.pos_draw_cached_cycles),
            cfg,
            draw_cache_counter: 0,
        })
    }

    async fn get_initial_rolls(
        cfg: &ConsensusConfig,
    ) -> Result<Vec<BTreeMap<Address, u64>>, ConsensusError> {
        Ok(serde_json::from_str::<Vec<BTreeMap<Address, u64>>>(
            &tokio::fs::read_to_string(&cfg.initial_rolls_path).await?,
        )?)
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
            final_roll_data: self
                .final_roll_data
                .iter()
                .map(|vec| {
                    vec.iter()
                        .map(|frtd| frtd.export())
                        .collect::<Vec<ExportFinalRollThreadData>>()
                })
                .collect(),
        }
    }

    pub async fn from_export(
        cfg: ConsensusConfig,
        export: ExportProofOfStake,
    ) -> Result<ProofOfStake, ConsensusError> {
        let final_roll_data: Vec<VecDeque<FinalRollThreadData>> = export
            .final_roll_data
            .into_iter()
            .map(|vec| {
                vec.into_iter()
                    .map(|frtd| FinalRollThreadData::from_export(frtd))
                    .collect::<VecDeque<FinalRollThreadData>>()
            })
            .collect();

        // if initial rolls are still needed for some threads, load them
        let initial_rolls = if final_roll_data
            .iter()
            .any(|v| v[0].cycle < cfg.pos_lock_cycles + cfg.pos_lock_cycles + 1)
        {
            Some(ProofOfStake::get_initial_rolls(&cfg).await?)
        } else {
            None
        };

        // generate object
        Ok(ProofOfStake {
            final_roll_data,
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
                for (addr, &n_rolls) in final_data.roll_count.iter() {
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
                for (addr, &n_rolls) in init_rolls.iter() {
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
        let cycle = slot.period / self.cfg.periods_per_cycle;
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
    pub fn note_final_blocks(&mut self, blocks: &HashMap<BlockId, ActiveBlock>) {
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
            let cur_last_final_period = self.final_roll_data[thread as usize][0]
                .last_final_slot
                .period;
            for period in (cur_last_final_period + 1)..=block_slot.period {
                let cycle = period / self.cfg.periods_per_cycle;
                let slot = Slot::new(period, thread);

                // if the cycle of the miss/block being processed is higher than the latest final block cycle
                // then create a new FinalRollThreadData representing this new cycle and push it at the front of final_roll_data[thread]
                // (step 1 in the spec)
                if cycle
                    > self.final_roll_data[thread as usize][0]
                        .last_final_slot
                        .period
                        / self.cfg.periods_per_cycle
                {
                    // the new FinalRollThreadData inherits from the roll_count of the previous cycle but has empty cycle_purchases, cycle_sales, rng_seed
                    let roll_count = self.final_roll_data[thread as usize][0].roll_count.clone();
                    self.final_roll_data[thread as usize].push_front(FinalRollThreadData {
                        cycle,
                        last_final_slot: slot.clone(),
                        cycle_purchases: HashMap::new(),
                        cycle_sales: HashMap::new(),
                        roll_count,
                        rng_seed: BitVec::new(),
                    });
                    // If final_roll_data becomes longer than pos_lookback_cycles+pos_lock_cycles+1, truncate it by removing the back elements
                    self.final_roll_data[thread as usize].truncate(
                        (self.cfg.pos_lookback_cycles + self.cfg.pos_lock_cycles + 2) as usize,
                    );
                }

                // apply the miss/block to the latest final_roll_data
                // (step 2 in the spec)
                let entry = &mut self.final_roll_data[thread as usize][0];
                // update the last_final_slot for the latest cycle
                entry.last_final_slot = slot.clone();
                // check if we are applying the block itself or a miss
                if period == block_slot.period {
                    // we are applying the block itself
                    // overwrite the cycle's roll_count,cycle_purchases,cycle_sales with the subsets from the block
                    entry.roll_count.extend(&a_block.roll_updates.roll_count);
                    entry
                        .cycle_purchases
                        .extend(&a_block.roll_updates.cycle_purchases);
                    entry.cycle_sales.extend(&a_block.roll_updates.cycle_sales);
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
                .final_roll_data
                .iter()
                .any(|v| v[0].cycle < self.cfg.pos_lock_cycles + self.cfg.pos_lock_cycles + 1)
            {
                self.initial_rolls = None;
            }
        }
    }

    pub fn get_last_final_block_cycle(&self, thread: u8) -> u64 {
        self.final_roll_data[thread as usize][0].cycle
    }

    pub fn get_final_roll_data(&self, cycle: u64, thread: u8) -> Option<&FinalRollThreadData> {
        let last_final_block_cycle = self.get_last_final_block_cycle(thread);
        if let Some(neg_relative_cycle) = last_final_block_cycle.checked_sub(cycle) {
            self.final_roll_data[thread as usize].get(neg_relative_cycle as usize)
        } else {
            None
        }
    }
}

impl FinalRollThreadData {
    fn export(&self) -> ExportFinalRollThreadData {
        ExportFinalRollThreadData {
            cycle: self.cycle,
            last_final_slot: self.last_final_slot,
            roll_count: self.roll_count.clone().into_iter().collect(),
            cycle_purchases: self.cycle_purchases.clone().into_iter().collect(),
            cycle_sales: self.cycle_sales.clone().into_iter().collect(),
            rng_seed: self.rng_seed.clone(),
        }
    }

    fn from_export(export: ExportFinalRollThreadData) -> FinalRollThreadData {
        FinalRollThreadData {
            cycle: export.cycle,
            last_final_slot: export.last_final_slot,
            roll_count: export.roll_count.into_iter().collect(),
            cycle_purchases: export.cycle_purchases.into_iter().collect(),
            cycle_sales: export.cycle_sales.into_iter().collect(),
            rng_seed: export.rng_seed,
        }
    }
}
