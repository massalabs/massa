use std::collections::{hash_map, BTreeMap, HashMap, VecDeque};

use bitvec::{order::Lsb0, prelude::BitVec};
use massa_hash::hash::Hash;
use massa_models::{
    active_block::ActiveBlock,
    prehash::{Map, Set},
    rolls::{RollCounts, RollUpdates},
    Address, Amount, BlockId, Slot, StakersCycleProductionStats,
};
use massa_signature::derive_public_key;
use num::rational::Ratio;
use rand::{distributions::Uniform, Rng, SeedableRng};
use rand_xoshiro::Xoshiro256PlusPlus;
use tracing::warn;

use crate::{
    error::POSResult, error::ProofOfStakeError, export_pos::ExportProofOfStake,
    settings::ProofOfStakeConfig, thread_cycle_state::ThreadCycleState,
};
type DrawCache = HashMap<u64, (usize, HashMap<Slot, (Address, Vec<Address>)>)>;

pub struct ProofOfStake {
    /// Config
    cfg: ProofOfStakeConfig,
    /// Index by thread and cycle number
    pub(crate) cycle_states: Vec<VecDeque<ThreadCycleState>>,
    /// Cycle draw cache: cycle_number => (counter, map(slot => (block_creator_addr, vec<endorsement_creator_addr>)))
    draw_cache: DrawCache,
    draw_cache_counter: usize,
    /// Initial rolls: we keep them as long as negative cycle draws are needed
    initial_rolls: Option<Vec<RollCounts>>,
    /// Initial seeds: they are lightweight, we always keep them
    /// the seed for cycle -N is obtained by hashing N times the value ConsensusConfig.initial_draw_seed
    /// the seeds are indexed from -1 to -N
    initial_seeds: Vec<Vec<u8>>,
    /// watched addresses
    watched_addresses: Set<Address>,
}

impl ProofOfStake {
    pub async fn new(
        cfg: ProofOfStakeConfig,
        genesis_block_ids: &[BlockId],
        boot_pos: Option<ExportProofOfStake>,
    ) -> POSResult<ProofOfStake> {
        let initial_seeds = ProofOfStake::generate_initial_seeds(&cfg);
        let draw_cache = HashMap::with_capacity(cfg.pos_draw_cached_cycles);
        let draw_cache_counter: usize = 0;

        let (cycle_states, initial_rolls) = if let Some(export) = boot_pos {
            // loading from bootstrap

            // if initial rolls are still needed for some threads, load them
            let initial_rolls = if export
                .cycle_states
                .iter()
                .any(|v| v[0].cycle < cfg.pos_lock_cycles + cfg.pos_lock_cycles + 1)
            {
                Some(ProofOfStake::get_initial_rolls(&cfg).await?)
            } else {
                None
            };

            (export.cycle_states, initial_rolls)
        } else {
            // initializing from scratch
            let mut cycle_states = Vec::with_capacity(cfg.thread_count as usize);
            let initial_rolls = ProofOfStake::get_initial_rolls(&cfg).await?;
            for (thread, thread_rolls) in initial_rolls.iter().enumerate() {
                // init thread history with one cycle
                let mut rng_seed = BitVec::<Lsb0, u8>::new();
                rng_seed.push(genesis_block_ids[thread].get_first_bit());
                let mut history = VecDeque::with_capacity(
                    (cfg.pos_lock_cycles + cfg.pos_lock_cycles + 2 + 1) as usize,
                );
                let thread_cycle_state = ThreadCycleState {
                    cycle: 0,
                    last_final_slot: Slot::new(0, thread as u8),
                    roll_count: thread_rolls.clone(),
                    cycle_updates: RollUpdates::default(),
                    rng_seed,
                    production_stats: Default::default(),
                };
                history.push_front(thread_cycle_state);
                cycle_states.push(history);
            }

            (cycle_states, Some(initial_rolls))
        };

        // generate object
        Ok(ProofOfStake {
            cycle_states,
            initial_rolls,
            initial_seeds,
            draw_cache,
            cfg,
            draw_cache_counter,
            watched_addresses: Set::<Address>::default(),
        })
    }

    pub fn set_watched_addresses(&mut self, addrs: Set<Address>) {
        self.watched_addresses = addrs;
    }

    /// active stakers count
    pub fn get_stakers_count(&self, target_cycle: u64) -> POSResult<u64> {
        let mut res: u64 = 0;
        for thread in 0..self.cfg.thread_count {
            res += self.get_lookback_roll_count(target_cycle, thread)?.0.len() as u64;
        }
        Ok(res)
    }

    async fn get_initial_rolls(cfg: &ProofOfStakeConfig) -> POSResult<Vec<RollCounts>> {
        let mut res = vec![BTreeMap::<Address, u64>::new(); cfg.thread_count as usize];
        let addrs_map = serde_json::from_str::<Map<Address, u64>>(
            &tokio::fs::read_to_string(&cfg.initial_rolls_path).await?,
        )?;
        for (addr, n_rolls) in addrs_map.into_iter() {
            res[addr.get_thread(cfg.thread_count) as usize].insert(addr, n_rolls);
        }
        Ok(res.into_iter().map(RollCounts).collect())
    }

    fn generate_initial_seeds(cfg: &ProofOfStakeConfig) -> Vec<Vec<u8>> {
        let mut cur_seed = cfg.initial_draw_seed.as_bytes().to_vec();
        let mut initial_seeds = Vec::with_capacity((cfg.pos_lookback_cycles + 1) as usize);
        for _ in 0..(cfg.pos_lookback_cycles + 1) {
            cur_seed = Hash::compute_from(&cur_seed).to_bytes().to_vec();
            initial_seeds.push(cur_seed.clone());
        }
        initial_seeds
    }

    pub fn get_next_selected_slot(&mut self, from_slot: Slot, address: Address) -> Option<Slot> {
        let mut cur_cycle = from_slot.get_cycle(self.cfg.periods_per_cycle);
        loop {
            let next_draw = match self.get_cycle_draws(cur_cycle) {
                Ok(draws) => draws,
                Err(_) => return None,
            }
            .iter()
            .filter(|(&k, (b_addr, _))| *b_addr == address && k > from_slot)
            .min_by_key(|(&k, _addr)| k);
            if let Some((next_slot, _next_addr)) = next_draw {
                return Some(*next_slot);
            }
            cur_cycle += 1;
        }
    }

    /// returns map slot -> ( block producer, endorsement producers)
    fn get_cycle_draws(
        &mut self,
        cycle: u64,
    ) -> POSResult<&HashMap<Slot, (Address, Vec<Address>)>> {
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
        let (cum_sum, rng_seed) = if cycle > self.cfg.pos_lookback_cycles {
            // nominal case: lookback after or at cycle 0
            let target_cycle = cycle - self.cfg.pos_lookback_cycles - 1;

            // get final data for all threads
            let mut rng_seed_bits = BitVec::<Lsb0, u8>::with_capacity(blocks_in_cycle);

            let mut cum_sum: Vec<(u64, Address)> = Vec::new(); // amount, thread, address
            let mut cum_sum_cursor = 0u64;
            for scan_thread in 0..self.cfg.thread_count {
                let final_data = self
                    .get_final_roll_data(target_cycle, scan_thread)
                    .ok_or_else(|| {
                        ProofOfStakeError::PosCycleUnavailable(format!(
                    "trying to get PoS draw rolls/seed for cycle {} thread {} which is unavailable",
                    target_cycle, scan_thread
                ))
                    })?;
                if !final_data.is_complete(self.cfg.periods_per_cycle) {
                    // the target cycle is not final yet
                    return Err(ProofOfStakeError::PosCycleUnavailable(format!("tryign to get PoS draw rolls/seed for cycle {} thread {} which is not finalized yet", target_cycle, scan_thread)));
                }
                rng_seed_bits.extend(&final_data.rng_seed);
                for (addr, &n_rolls) in final_data.roll_count.0.iter() {
                    if n_rolls == 0 {
                        continue;
                    }
                    cum_sum_cursor += n_rolls;
                    cum_sum.push((cum_sum_cursor, *addr));
                }
            }
            // compute the RNG seed from the seed bits
            let rng_seed = Hash::compute_from(&rng_seed_bits.into_vec())
                .to_bytes()
                .to_vec();

            (cum_sum, rng_seed)
        } else {
            // special case: lookback before cycle 0

            // get initial rolls
            let mut cum_sum: Vec<(u64, Address)> = Vec::new(); // amount, thread, address
            let mut cum_sum_cursor = 0u64;
            for scan_thread in 0..self.cfg.thread_count {
                let init_rolls = &self.initial_rolls.as_ref().ok_or_else( ||
                    ProofOfStakeError::PosCycleUnavailable(format!(
                    "trying to get PoS initial draw rolls/seed for negative cycle at thread {}, which is unavailable",
                    scan_thread
                )))?[scan_thread as usize];
                for (addr, &n_rolls) in init_rolls.0.iter() {
                    if n_rolls == 0 {
                        continue;
                    }
                    cum_sum_cursor += n_rolls;
                    cum_sum.push((cum_sum_cursor, *addr));
                }
            }

            // get RNG seed
            let seed_idx = self.cfg.pos_lookback_cycles - cycle;
            let rng_seed = self.initial_seeds[seed_idx as usize].clone();

            (cum_sum, rng_seed)
        };
        let cum_sum_max = cum_sum
            .last()
            .ok_or_else(|| {
                ProofOfStakeError::ContainerInconsistency("draw cum_sum is empty".into())
            })?
            .0;

        // init RNG
        let mut rng = Xoshiro256PlusPlus::from_seed(rng_seed.try_into().map_err(|_| {
            ProofOfStakeError::ContainerInconsistency(
                "could not seed RNG with computed seed".into(),
            )
        })?);

        // perform draws
        let distribution = Uniform::new(0, cum_sum_max);
        let mut draws: HashMap<Slot, (Address, Vec<Address>)> =
            HashMap::with_capacity(blocks_in_cycle);
        let cycle_first_period = cycle * self.cfg.periods_per_cycle;
        let cycle_last_period = (cycle + 1) * self.cfg.periods_per_cycle - 1;
        if cycle_first_period == 0 {
            // genesis slots: force block creator and endorsement creator address draw
            let genesis_addr = Address::from_public_key(&derive_public_key(&self.cfg.genesis_key));
            for draw_thread in 0..self.cfg.thread_count {
                draws.insert(
                    Slot::new(0, draw_thread),
                    (
                        genesis_addr,
                        vec![genesis_addr; self.cfg.endorsement_count as usize],
                    ),
                );
            }
        }
        for draw_period in cycle_first_period..=cycle_last_period {
            if draw_period == 0 {
                // do not draw genesis again
                continue;
            }
            for draw_thread in 0..self.cfg.thread_count {
                let mut res = Vec::with_capacity(self.cfg.endorsement_count as usize + 1);
                // draw block creator and endorsers with the same probabilities
                for _ in 0..(self.cfg.endorsement_count + 1) {
                    let sample = rng.sample(&distribution);

                    // locate the draw in the cum_sum through binary search
                    let found_index =
                        match cum_sum.binary_search_by_key(&sample, |(c_sum, _)| *c_sum) {
                            Ok(idx) => idx + 1,
                            Err(idx) => idx,
                        };
                    let (_sum, found_addr) = cum_sum[found_index];
                    res.push(found_addr)
                }

                draws.insert(
                    Slot::new(draw_period, draw_thread),
                    (res[0], res[1..].to_vec()),
                );
            }
        }

        // add new cache element
        Ok(&self
            .draw_cache
            .entry(cycle)
            .or_insert((self.draw_cache_counter, draws))
            .1)
    }

    pub fn draw_endorsement_producers(&mut self, slot: Slot) -> POSResult<Vec<Address>> {
        Ok(self.draw(slot)?.1)
    }

    pub fn draw_block_producer(&mut self, slot: Slot) -> POSResult<Address> {
        Ok(self.draw(slot)?.0)
    }

    /// returns (block producers, vec<endorsement producers>)
    fn draw(&mut self, slot: Slot) -> POSResult<(Address, Vec<Address>)> {
        let cycle = slot.get_cycle(self.cfg.periods_per_cycle);
        let cycle_draws = self.get_cycle_draws(cycle)?;
        Ok(cycle_draws
            .get(&slot)
            .ok_or_else(|| {
                ProofOfStakeError::ContainerInconsistency(format!(
                    "draw cycle computed for cycle {} but slot {} absent",
                    cycle, slot
                ))
            })?
            .clone())
    }

    /// Update internal states after a set of blocks become final
    /// see /consensus/pos.md#when-a-block-b-in-thread-tau-and-cycle-n-becomes-final
    pub fn note_final_blocks(&mut self, blocks: Map<BlockId, &ActiveBlock>) -> POSResult<()> {
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
                        last_final_slot: slot,
                        cycle_updates: RollUpdates::default(),
                        roll_count,
                        rng_seed: BitVec::<Lsb0, u8>::new(),
                        production_stats: Default::default(),
                    });
                    // If cycle_states becomes longer than pos_lookback_cycles+pos_lock_cycles+1, truncate it by removing the back elements
                    self.cycle_states[thread as usize].truncate(
                        (self.cfg.pos_lookback_cycles + self.cfg.pos_lock_cycles + 2) as usize,
                    );
                }

                // update production_stats
                // (step 2 in the spec)
                if period == block_slot.period {
                    // we are applying the block itself
                    let last_final_block_cycle = self.get_last_final_block_cycle(thread);
                    for (evt_period, evt_addr, evt_ok) in a_block.production_events.iter() {
                        let evt_slot = Slot::new(*evt_period, thread);
                        let evt_cycle = evt_slot.get_cycle(self.cfg.periods_per_cycle);
                        if !evt_ok && self.watched_addresses.contains(evt_addr) {
                            warn!(
                                "address {} missed a production opportunity at slot {} (cycle {})",
                                evt_addr, evt_slot, evt_cycle
                            );
                        }
                        if let Some(neg_relative_cycle) =
                            last_final_block_cycle.checked_sub(evt_cycle)
                        {
                            if let Some(entry) = self.cycle_states[thread as usize]
                                .get_mut(neg_relative_cycle as usize)
                            {
                                match entry.production_stats.entry(*evt_addr) {
                                    hash_map::Entry::Occupied(mut occ) => {
                                        let cur_val = *occ.get();
                                        if *evt_ok {
                                            *occ.get_mut() = (cur_val.0 + 1, cur_val.1);
                                        } else {
                                            *occ.get_mut() = (cur_val.0, cur_val.1 + 1);
                                        }
                                    }
                                    hash_map::Entry::Vacant(vac) => {
                                        if *evt_ok {
                                            vac.insert((1, 0));
                                        } else {
                                            vac.insert((0, 1));
                                        }
                                    }
                                }
                            }
                        }
                    }
                }

                // apply the miss/block to the latest cycle_states
                // (step 3 in the spec)
                let entry = &mut self.cycle_states[thread as usize][0];
                // update the last_final_slot for the latest cycle
                entry.last_final_slot = slot;
                // check if we are applying the block itself or a miss
                if period == block_slot.period {
                    // we are applying the block itself
                    // compensations/deactivations have already been taken into account within the block and converted to ledger changes so we ignore them here
                    entry.cycle_updates.chain(&a_block.roll_updates)?;
                    entry.roll_count.apply_updates(&a_block.roll_updates)?;
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
        if self.initial_rolls.is_some()
            && !self
                .cycle_states
                .iter()
                .any(|v| v[0].cycle < self.cfg.pos_lock_cycles + self.cfg.pos_lock_cycles + 1)
        {
            self.initial_rolls = None;
        }

        Ok(())
    }

    pub fn get_stakers_production_stats(
        &self,
        addrs: &Set<Address>,
    ) -> Vec<StakersCycleProductionStats> {
        let mut res: HashMap<u64, StakersCycleProductionStats> = HashMap::new();
        let mut completeness: HashMap<u64, u8> = HashMap::new();
        for thread_info in self.cycle_states.iter() {
            for thread_cycle_info in thread_info.iter() {
                let cycle = thread_cycle_info.cycle;
                let thread_cycle_complete =
                    thread_cycle_info.is_complete(self.cfg.periods_per_cycle);

                let cycle_entry = res
                    .entry(cycle)
                    .or_insert_with(|| StakersCycleProductionStats {
                        cycle,
                        is_final: false,
                        ok_nok_counts: Map::default(),
                    });

                cycle_entry.is_final = if thread_cycle_complete {
                    *completeness
                        .entry(cycle)
                        .and_modify(|n| *n += 1)
                        .or_insert(1)
                        == self.cfg.thread_count
                } else {
                    false
                };

                for addr in addrs {
                    let (n_ok, n_nok) = thread_cycle_info
                        .production_stats
                        .get(addr)
                        .unwrap_or(&(0, 0));
                    cycle_entry
                        .ok_nok_counts
                        .entry(*addr)
                        .and_modify(|(p_ok, p_nok)| {
                            *p_ok += n_ok;
                            *p_nok += n_nok;
                        })
                        .or_insert_with(|| (*n_ok, *n_nok));
                }
            }
        }
        res.into_values().collect()
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

    /// returns the roll sell credit amount (in coins) for each credited address
    pub fn get_roll_sell_credit(&self, cycle: u64, thread: u8) -> POSResult<Map<Address, Amount>> {
        let mut res = Map::default();
        if let Some(target_cycle) =
            cycle.checked_sub(self.cfg.pos_lookback_cycles + self.cfg.pos_lock_cycles + 1)
        {
            let roll_data = self
                .get_final_roll_data(target_cycle, thread)
                .ok_or(ProofOfStakeError::NotFinalRollError)?;
            if !roll_data.is_complete(self.cfg.periods_per_cycle) {
                return Err(ProofOfStakeError::NotFinalRollError); // target_cycle not completely final
            }
            for (addr, update) in roll_data.cycle_updates.0.iter() {
                let sale_delta = update.roll_sales.saturating_sub(update.roll_purchases);
                if sale_delta > 0 {
                    res.insert(
                        *addr,
                        self.cfg
                            .roll_price
                            .checked_mul_u64(sale_delta)
                            .ok_or(ProofOfStakeError::RollOverflowError)?,
                    );
                }
            }
        }
        Ok(res)
    }

    /// returns the list of addresses whose rolls need to be deactivated
    pub fn get_roll_deactivations(
        &self,
        cycle: u64,
        address_thread: u8,
    ) -> POSResult<Set<Address>> {
        // compute target cycle
        if cycle <= self.cfg.pos_lookback_cycles {
            // no lookback cycles yet: do not deactivate anyone
            return Ok(Set::<Address>::default());
        }
        let target_cycle = cycle - self.cfg.pos_lookback_cycles - 1;

        // get roll data from all threads for addresses belonging to address_thread
        let mut addr_stats: Map<Address, (u64, u64)> = Map::default();
        for thread in 0..self.cfg.thread_count {
            // get roll data
            let roll_data = self
                .get_final_roll_data(target_cycle, thread)
                .ok_or(ProofOfStakeError::NotFinalRollError)?;
            if !roll_data.is_complete(self.cfg.periods_per_cycle) {
                return Err(ProofOfStakeError::NotFinalRollError); // target_cycle not completely final
            }
            // accumulate counters
            for (addr, (n_ok, n_nok)) in roll_data.production_stats.iter() {
                if addr.get_thread(self.cfg.thread_count) != address_thread {
                    continue;
                }
                match addr_stats.entry(*addr) {
                    hash_map::Entry::Occupied(mut occ) => {
                        let cur = *occ.get();
                        *occ.get_mut() = (cur.0 + n_ok, cur.1 + n_nok);
                    }
                    hash_map::Entry::Vacant(vac) => {
                        vac.insert((*n_ok, *n_nok));
                    }
                }
            }
        }
        // list addresses with bad stats
        let res: Set<Address> = addr_stats
            .into_iter()
            .filter_map(|(addr, (ok_count, nok_count))| {
                if ok_count + nok_count == 0 {
                    return None;
                }
                let miss_ratio = Ratio::new(nok_count, ok_count + nok_count);
                if miss_ratio > self.cfg.pos_miss_rate_deactivation_threshold {
                    return Some(addr);
                }
                None
            })
            .collect();

        for alert_addr in res.intersection(&self.watched_addresses) {
            warn!("address {} is subject to an implicit roll sale at cycle {}. Check the stability of your node/connection to avoid further misses, then buy rolls again.", alert_addr, cycle);
        }

        Ok(res)
    }

    /// gets the number of locked rolls at a given slot for a set of addresses
    pub fn get_locked_roll_count(
        &self,
        cycle: u64,
        thread: u8,
        addrs: &Set<Address>,
    ) -> Map<Address, u64> {
        if cycle < 1 + self.cfg.pos_lookback_cycles {
            return Map::default();
        }
        let start_cycle = cycle
            .saturating_sub(self.cfg.pos_lookback_cycles)
            .saturating_sub(self.cfg.pos_lock_cycles);
        let end_cycle = cycle - self.cfg.pos_lookback_cycles - 1;
        let mut res: Map<Address, u64> = Map::default();
        for origin_cycle in start_cycle..=end_cycle {
            if let Some(origin_state) = self.get_final_roll_data(origin_cycle, thread) {
                for addr in addrs.iter() {
                    if let Some(updates) = origin_state.cycle_updates.0.get(addr) {
                        let compensated_sales =
                            updates.roll_sales.saturating_sub(updates.roll_purchases);
                        if compensated_sales == 0 {
                            continue;
                        }
                        res.entry(*addr)
                            .and_modify(|v| *v += compensated_sales)
                            .or_insert(compensated_sales);
                    }
                }
            }
        }
        res
    }

    /// Gets cycle in which we are drawing at source_cycle
    pub fn get_lookback_roll_count(&self, source_cycle: u64, thread: u8) -> POSResult<&RollCounts> {
        if source_cycle > self.cfg.pos_lookback_cycles {
            // nominal case: lookback after or at cycle 0
            let target_cycle = source_cycle - self.cfg.pos_lookback_cycles - 1;
            if let Some(state) = self.get_final_roll_data(target_cycle, thread) {
                if !state.is_complete(self.cfg.periods_per_cycle) {
                    return Err(ProofOfStakeError::PosCycleUnavailable(
                        "target cycle incomplete".to_string(),
                    ));
                }
                Ok(&state.roll_count)
            } else {
                Err(ProofOfStakeError::PosCycleUnavailable(
                    "target cycle unavailable".to_string(),
                ))
            }
        } else if let Some(init) = &self.initial_rolls {
            Ok(&init[thread as usize])
        } else {
            Err(ProofOfStakeError::PosCycleUnavailable(
                "negative cycle unavailable".to_string(),
            ))
        }
    }
}
