// Copyright (c) 2021 MASSA LABS <info@massa.net>

use crate::error::ConsensusError;
use crate::{block_graph::ActiveBlock, ConsensusConfig};
use bitvec::prelude::*;
use massa_hash::hash::Hash;
use models::address::{AddressHashMap, AddressHashSet};
use models::hhasher::BuildHHasher;
use models::{
    array_from_slice, with_serialization_context, Address, Amount, BlockHashMap, BlockId,
    DeserializeCompact, DeserializeVarInt, ModelsError, Operation, OperationType, SerializeCompact,
    SerializeVarInt, Slot, StakersCycleProductionStats, ADDRESS_SIZE_BYTES,
};
use num::rational::Ratio;
use num::Integer;
use rand::distributions::Uniform;
use rand::Rng;
use rand_xoshiro::rand_core::SeedableRng;
use rand_xoshiro::Xoshiro256PlusPlus;
use serde::{Deserialize, Serialize};
use signature::derive_public_key;
use std::collections::{btree_map, hash_map, BTreeMap, HashMap, VecDeque};
use std::convert::TryInto;
use tracing::warn;

#[derive(Copy, Clone, Debug, Serialize, Deserialize)]
pub struct RollCompensation(pub u64);

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RollUpdate {
    pub roll_purchases: u64,
    pub roll_sales: u64,
    // Here is space for registering any denunciations/resets
}

impl SerializeCompact for RollUpdate {
    fn to_bytes_compact(&self) -> Result<Vec<u8>, ModelsError> {
        let mut res: Vec<u8> = Vec::new();

        // roll purchases
        res.extend(self.roll_purchases.to_varint_bytes());

        // roll sales
        res.extend(self.roll_sales.to_varint_bytes());

        Ok(res)
    }
}

impl DeserializeCompact for RollUpdate {
    fn from_bytes_compact(buffer: &[u8]) -> Result<(Self, usize), ModelsError> {
        let mut cursor = 0usize;

        // roll purchases
        let (roll_purchases, delta) = u64::from_varint_bytes(&buffer[cursor..])?;
        cursor += delta;

        // roll sales
        let (roll_sales, delta) = u64::from_varint_bytes(&buffer[cursor..])?;
        cursor += delta;

        Ok((
            RollUpdate {
                roll_purchases,
                roll_sales,
            },
            cursor,
        ))
    }
}

impl RollUpdate {
    /// chain two roll updates, compensate and return compensation count
    fn chain(&mut self, change: &Self) -> Result<RollCompensation, ConsensusError> {
        let compensation_other = std::cmp::min(change.roll_purchases, change.roll_sales);
        self.roll_purchases = self
            .roll_purchases
            .checked_add(change.roll_purchases - compensation_other)
            .ok_or(ConsensusError::InvalidRollUpdate(
                "roll_purchases overflow in RollUpdate::chain".into(),
            ))?;
        self.roll_sales = self
            .roll_sales
            .checked_add(change.roll_sales - compensation_other)
            .ok_or(ConsensusError::InvalidRollUpdate(
                "roll_sales overflow in RollUpdate::chain".into(),
            ))?;

        let compensation_self = self.compensate().0;

        let compensation_total = compensation_other.checked_add(compensation_self).ok_or(
            ConsensusError::InvalidRollUpdate("compensation overflow in RollUpdate::chain".into()),
        )?;
        Ok(RollCompensation(compensation_total))
    }

    /// compensate a roll update, return compensation count
    pub fn compensate(&mut self) -> RollCompensation {
        let compensation = std::cmp::min(self.roll_purchases, self.roll_sales);
        self.roll_purchases -= compensation;
        self.roll_sales -= compensation;
        RollCompensation(compensation)
    }

    pub fn is_nil(&self) -> bool {
        self.roll_purchases == 0 && self.roll_sales == 0
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, Default)]
pub struct RollUpdates(pub AddressHashMap<RollUpdate>);

impl RollUpdates {
    pub fn get_involved_addresses(&self) -> AddressHashSet {
        self.0.keys().copied().collect()
    }

    /// chains with another RollUpdates, compensates and returns compensations
    pub fn chain(
        &mut self,
        updates: &RollUpdates,
    ) -> Result<AddressHashMap<RollCompensation>, ConsensusError> {
        let mut res = AddressHashMap::default();
        for (addr, update) in updates.0.iter() {
            res.insert(*addr, self.apply(addr, update)?);
            // remove if nil
            if let hash_map::Entry::Occupied(occ) = self.0.entry(*addr) {
                if occ.get().is_nil() {
                    occ.remove();
                }
            }
        }
        Ok(res)
    }

    /// applies a RollUpdate, compensates and returns compensation
    pub fn apply(
        &mut self,
        addr: &Address,
        update: &RollUpdate,
    ) -> Result<RollCompensation, ConsensusError> {
        if update.is_nil() {
            return Ok(RollCompensation(0));
        }
        match self.0.entry(*addr) {
            hash_map::Entry::Occupied(mut occ) => occ.get_mut().chain(update),
            hash_map::Entry::Vacant(vac) => {
                let mut compensated_update = update.clone();
                let compensation = compensated_update.compensate();
                vac.insert(compensated_update);
                Ok(compensation)
            }
        }
    }

    /// get the roll update for a subset of addresses
    pub fn clone_subset(&self, addrs: &AddressHashSet) -> Self {
        Self(
            addrs
                .iter()
                .filter_map(|addr| self.0.get(addr).map(|v| (*addr, v.clone())))
                .collect(),
        )
    }

    /// merge another roll updates into self, overwriting existing data
    /// addrs that are in not other are removed from self
    pub fn sync_from(&mut self, addrs: &AddressHashSet, mut other: RollUpdates) {
        for addr in addrs.iter() {
            if let Some(new_val) = other.0.remove(addr) {
                self.0.insert(*addr, new_val);
            } else {
                self.0.remove(addr);
            }
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, Default)]
pub struct RollCounts(pub BTreeMap<Address, u64>);

impl RollCounts {
    pub fn new() -> Self {
        RollCounts(BTreeMap::new())
    }

    pub fn len(&self) -> usize {
        self.0.len()
    }

    /// applies RollUpdates to self with compensations
    pub fn apply_updates(&mut self, updates: &RollUpdates) -> Result<(), ConsensusError> {
        for (addr, update) in updates.0.iter() {
            match self.0.entry(*addr) {
                btree_map::Entry::Occupied(mut occ) => {
                    let cur_val = *occ.get();
                    if update.roll_purchases >= update.roll_sales {
                        *occ.get_mut() = cur_val
                            .checked_add(update.roll_purchases - update.roll_sales)
                            .ok_or(ConsensusError::InvalidRollUpdate(
                                "overflow while incrementing roll count".into(),
                            ))?;
                    } else {
                        *occ.get_mut() = cur_val
                            .checked_sub(update.roll_sales - update.roll_purchases)
                            .ok_or(ConsensusError::InvalidRollUpdate(
                                "underflow while decrementing roll count".into(),
                            ))?;
                    }
                    if *occ.get() == 0 {
                        // remove if 0
                        occ.remove();
                    }
                }
                btree_map::Entry::Vacant(vac) => {
                    if update.roll_purchases >= update.roll_sales {
                        if update.roll_purchases > update.roll_sales {
                            // ignore if 0
                            vac.insert(update.roll_purchases - update.roll_sales);
                        }
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

    /// get roll counts for a subset of addresses.
    pub fn clone_subset(&self, addrs: &AddressHashSet) -> Self {
        Self(
            addrs
                .iter()
                .filter_map(|addr| self.0.get(addr).map(|v| (*addr, *v)))
                .collect(),
        )
    }

    /// merge another roll counts into self, overwriting existing data
    /// addrs that are in not other are removed from self
    pub fn sync_from(&mut self, addrs: &AddressHashSet, mut other: RollCounts) {
        for addr in addrs.iter() {
            if let Some(new_val) = other.0.remove(addr) {
                self.0.insert(*addr, new_val);
            } else {
                self.0.remove(addr);
            }
        }
    }
}

/// Roll specific method on operation
pub trait OperationRollInterface {
    /// get roll related modifications
    fn get_roll_updates(&self) -> Result<RollUpdates, ConsensusError>;
}

impl OperationRollInterface for Operation {
    fn get_roll_updates(&self) -> Result<RollUpdates, ConsensusError> {
        let mut res = RollUpdates::default();
        match self.content.op {
            OperationType::Transaction { .. } => {}
            OperationType::RollBuy { roll_count } => {
                res.apply(
                    &Address::from_public_key(&self.content.sender_public_key)?,
                    &RollUpdate {
                        roll_purchases: roll_count,
                        roll_sales: 0,
                    },
                )?;
            }
            OperationType::RollSell { roll_count } => {
                res.apply(
                    &Address::from_public_key(&self.content.sender_public_key)?,
                    &RollUpdate {
                        roll_purchases: 0,
                        roll_sales: roll_count,
                    },
                )?;
            }
        }
        Ok(res)
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ThreadCycleState {
    /// Cycle number
    pub cycle: u64,
    /// Last final slot (can be a miss)
    pub last_final_slot: Slot,
    /// Number of rolls an address has
    pub roll_count: RollCounts,
    /// Cycle roll updates
    pub cycle_updates: RollUpdates,
    /// Used to seed the random selector at each cycle
    pub rng_seed: BitVec<Lsb0, u8>,
    /// Per-address production statistics (ok_count, nok_count)
    pub production_stats: AddressHashMap<(u64, u64)>,
}

impl ThreadCycleState {
    /// returns true if all slots of this cycle for this thread are final
    fn is_complete(&self, periods_per_cycle: u64) -> bool {
        self.last_final_slot.period == (self.cycle + 1) * periods_per_cycle - 1
    }
}

pub struct ProofOfStake {
    /// Config
    cfg: ConsensusConfig,
    /// Index by thread and cycle number
    cycle_states: Vec<VecDeque<ThreadCycleState>>,
    /// Cycle draw cache: cycle_number => (counter, map(slot => (block_creator_addr, vec<endorsement_creator_addr>)))
    draw_cache: HashMap<u64, (usize, HashMap<Slot, (Address, Vec<Address>)>)>,
    draw_cache_counter: usize,
    /// Initial rolls: we keep them as long as negative cycle draws are needed
    initial_rolls: Option<Vec<RollCounts>>,
    /// Initial seeds: they are lightweight, we always keep them
    /// the seed for cycle -N is obtained by hashing N times the value ConsensusConfig.initial_draw_seed
    /// the seeds are indexed from -1 to -N
    initial_seeds: Vec<Vec<u8>>,
    /// watched addresses
    watched_addresses: AddressHashSet,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExportProofOfStake {
    /// Index by thread and cycle number
    pub cycle_states: Vec<VecDeque<ThreadCycleState>>,
}

impl SerializeCompact for ExportProofOfStake {
    fn to_bytes_compact(&self) -> Result<Vec<u8>, ModelsError> {
        let mut res: Vec<u8> = Vec::new();
        for thread_lst in self.cycle_states.iter() {
            let cycle_count: u32 = thread_lst.len().try_into().map_err(|err| {
                ModelsError::SerializeError(format!(
                    "too many cycles when serializing ExportProofOfStake: {}",
                    err
                ))
            })?;
            res.extend(cycle_count.to_varint_bytes());
            for itm in thread_lst.iter() {
                res.extend(itm.to_bytes_compact()?);
            }
        }
        Ok(res)
    }
}

impl DeserializeCompact for ExportProofOfStake {
    fn from_bytes_compact(buffer: &[u8]) -> Result<(Self, usize), ModelsError> {
        let (thread_count, max_cycles) = with_serialization_context(|context| {
            (context.parent_count, context.max_bootstrap_pos_cycles)
        });
        let mut cursor = 0usize;

        let mut cycle_states = Vec::with_capacity(thread_count as usize);
        for thread in 0..thread_count {
            let (n_cycles, delta) = u32::from_varint_bytes(&buffer[cursor..])?;
            cursor += delta;
            if n_cycles == 0 || n_cycles > max_cycles {
                return Err(ModelsError::SerializeError(
                    "number of cycles invalid when deserializing ExportProofOfStake".into(),
                ));
            }
            cycle_states.push(VecDeque::with_capacity(n_cycles as usize));
            for _ in 0..n_cycles {
                let (thread_cycle_state, delta) =
                    ThreadCycleState::from_bytes_compact(&buffer[cursor..])?;
                cursor += delta;
                cycle_states[thread as usize].push_back(thread_cycle_state);
            }
        }
        Ok((ExportProofOfStake { cycle_states }, cursor))
    }
}

impl SerializeCompact for ThreadCycleState {
    fn to_bytes_compact(&self) -> Result<Vec<u8>, ModelsError> {
        let mut res: Vec<u8> = Vec::new();

        // cycle
        res.extend(self.cycle.to_varint_bytes());

        // last final slot
        res.extend(self.last_final_slot.to_bytes_compact()?);

        // roll count
        let n_entries: u32 = self.roll_count.0.len().try_into().map_err(|err| {
            ModelsError::SerializeError(format!(
                "too many entries when serializing ExportThreadCycleState roll_count: {}",
                err
            ))
        })?;
        res.extend(n_entries.to_varint_bytes());
        for (addr, n_rolls) in self.roll_count.0.iter() {
            res.extend(addr.to_bytes());
            res.extend(n_rolls.to_varint_bytes());
        }

        // cycle updates
        let n_entries: u32 = self.cycle_updates.0.len().try_into().map_err(|err| {
            ModelsError::SerializeError(format!(
                "too many entries when serializing ExportThreadCycleState cycle_updates: {}",
                err
            ))
        })?;
        res.extend(n_entries.to_varint_bytes());
        for (addr, updates) in self.cycle_updates.0.iter() {
            res.extend(addr.to_bytes());
            res.extend(updates.to_bytes_compact()?);
        }

        // rng seed
        let n_entries: u32 = self.rng_seed.len().try_into().map_err(|err| {
            ModelsError::SerializeError(format!(
                "too many entries when serializing ExportThreadCycleState rng_seed: {}",
                err
            ))
        })?;
        res.extend(n_entries.to_varint_bytes());
        res.extend(self.rng_seed.clone().into_vec());

        // production stats
        let n_entries: u32 = self.production_stats.len().try_into().map_err(|err| {
            ModelsError::SerializeError(format!(
                "too many entries when serializing ExportThreadCycleState production_stats: {}",
                err
            ))
        })?;
        res.extend(n_entries.to_varint_bytes());
        for (addr, (ok_count, nok_count)) in self.production_stats.iter() {
            res.extend(addr.to_bytes());
            res.extend(ok_count.to_varint_bytes());
            res.extend(nok_count.to_varint_bytes());
        }

        Ok(res)
    }
}

impl DeserializeCompact for ThreadCycleState {
    fn from_bytes_compact(buffer: &[u8]) -> Result<(Self, usize), ModelsError> {
        let max_entries = with_serialization_context(|context| (context.max_bootstrap_pos_entries));
        let mut cursor = 0usize;

        // cycle
        let (cycle, delta) = u64::from_varint_bytes(&buffer[cursor..])?;
        cursor += delta;

        // last final slot
        let (last_final_slot, delta) = Slot::from_bytes_compact(&buffer[cursor..])?;
        cursor += delta;

        // roll count
        let (n_entries, delta) = u32::from_varint_bytes(&buffer[cursor..])?;
        cursor += delta;
        if n_entries > max_entries {
            return Err(ModelsError::SerializeError(
                "invalid number entries when deserializing ExportThreadCycleStat roll_count".into(),
            ));
        }
        let mut roll_count = RollCounts::default();
        for _ in 0..n_entries {
            let addr = Address::from_bytes(&array_from_slice(&buffer[cursor..])?)?;
            cursor += ADDRESS_SIZE_BYTES;
            let (rolls, delta) = u64::from_varint_bytes(&buffer[cursor..])?;
            cursor += delta;
            roll_count.0.insert(addr, rolls);
        }

        // cycle updates
        let (n_entries, delta) = u32::from_varint_bytes(&buffer[cursor..])?;
        cursor += delta;
        if n_entries > max_entries {
            return Err(ModelsError::SerializeError(
                "invalid number entries when deserializing ExportThreadCycleStat cycle_updates"
                    .into(),
            ));
        }
        let mut cycle_updates = RollUpdates(AddressHashMap::with_capacity_and_hasher(
            n_entries as usize,
            BuildHHasher::default(),
        ));
        for _ in 0..n_entries {
            let addr = Address::from_bytes(&array_from_slice(&buffer[cursor..])?)?;
            cursor += ADDRESS_SIZE_BYTES;
            let (update, delta) = RollUpdate::from_bytes_compact(&buffer[cursor..])?;
            cursor += delta;
            cycle_updates.0.insert(addr, update);
        }

        // rng seed
        let (n_entries, delta) = u32::from_varint_bytes(&buffer[cursor..])?;
        cursor += delta;
        if n_entries > max_entries {
            return Err(ModelsError::SerializeError(
                "invalid number entries when deserializing ExportThreadCycleStat rng_seed".into(),
            ));
        }
        let bits_u8_len = n_entries.div_ceil(&u8::BITS) as usize;
        if buffer[cursor..].len() < bits_u8_len {
            return Err(ModelsError::SerializeError(
                "too few remaining bytes when deserializing ExportThreadCycleStat rng_seed".into(),
            ));
        }
        let mut rng_seed: BitVec<Lsb0, u8> = BitVec::try_from_vec(buffer[cursor..(cursor+bits_u8_len)].to_vec())
            .map_err(|_| ModelsError::SerializeError("error in bitvec conversion during deserialization of ExportThreadCycleStat rng_seed".into()))?;
        rng_seed.truncate(n_entries as usize);
        if rng_seed.len() != n_entries as usize {
            return Err(ModelsError::SerializeError(
                "incorrect resulting size when deserializing ExportThreadCycleStat rng_seed".into(),
            ));
        }
        cursor += rng_seed.elements();

        // production stats
        let (n_entries, delta) = u32::from_varint_bytes(&buffer[cursor..])?;
        cursor += delta;
        if n_entries > max_entries {
            return Err(ModelsError::SerializeError(
                "invalid number entries when deserializing ExportThreadCycleStat production_stats"
                    .into(),
            ));
        }
        let mut production_stats =
            AddressHashMap::with_capacity_and_hasher(n_entries as usize, BuildHHasher::default());
        for _ in 0..n_entries {
            let addr = Address::from_bytes(&array_from_slice(&buffer[cursor..])?)?;
            cursor += ADDRESS_SIZE_BYTES;
            let (ok_count, delta) = u64::from_varint_bytes(&buffer[cursor..])?;
            cursor += delta;
            let (nok_count, delta) = u64::from_varint_bytes(&buffer[cursor..])?;
            cursor += delta;
            production_stats.insert(addr, (ok_count, nok_count));
        }

        // return struct
        Ok((
            ThreadCycleState {
                cycle,
                last_final_slot,
                roll_count,
                cycle_updates,
                rng_seed,
                production_stats,
            },
            cursor,
        ))
    }
}

impl ProofOfStake {
    pub async fn new(
        cfg: ConsensusConfig,
        genesis_block_ids: &[BlockId],
        boot_pos: Option<ExportProofOfStake>,
    ) -> Result<ProofOfStake, ConsensusError> {
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
            // iinitializing from scratch

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
            watched_addresses: AddressHashSet::default(),
        })
    }

    pub fn set_watched_addresses(&mut self, addrs: AddressHashSet) {
        self.watched_addresses = addrs;
    }

    /// active stakers count
    pub fn get_stakers_count(&self, target_cycle: u64) -> Result<u64, ConsensusError> {
        let mut res: u64 = 0;
        for thread in 0..self.cfg.thread_count {
            res += self.get_lookback_roll_count(target_cycle, thread)?.0.len() as u64;
        }
        Ok(res)
    }

    async fn get_initial_rolls(cfg: &ConsensusConfig) -> Result<Vec<RollCounts>, ConsensusError> {
        let mut res = vec![BTreeMap::<Address, u64>::new(); cfg.thread_count as usize];
        let addrs_map = serde_json::from_str::<AddressHashMap<u64>>(
            &tokio::fs::read_to_string(&cfg.initial_rolls_path).await?,
        )?;
        for (addr, n_rolls) in addrs_map.into_iter() {
            res[addr.get_thread(cfg.thread_count) as usize].insert(addr, n_rolls);
        }
        Ok(res.into_iter().map(RollCounts).collect())
    }

    fn generate_initial_seeds(cfg: &ConsensusConfig) -> Vec<Vec<u8>> {
        let mut cur_seed = cfg.initial_draw_seed.as_bytes().to_vec();
        let mut initial_seeds = Vec::with_capacity((cfg.pos_lookback_cycles + 1) as usize);
        for _ in 0..(cfg.pos_lookback_cycles + 1) {
            cur_seed = Hash::from(&cur_seed).to_bytes().to_vec();
            initial_seeds.push(cur_seed.clone());
        }
        initial_seeds
    }

    pub fn export(&self) -> ExportProofOfStake {
        ExportProofOfStake {
            cycle_states: self.cycle_states.clone(),
        }
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
    ) -> Result<&HashMap<Slot, (Address, Vec<Address>)>, ConsensusError> {
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
                        ConsensusError::PosCycleUnavailable(format!(
                    "trying to get PoS draw rolls/seed for cycle {} thread {} which is unavailable",
                    target_cycle, scan_thread
                ))
                    })?;
                if !final_data.is_complete(self.cfg.periods_per_cycle) {
                    // the target cycle is not final yet
                    return Err(ConsensusError::PosCycleUnavailable(format!("tryign to get PoS draw rolls/seed for cycle {} thread {} which is not finalized yet", target_cycle, scan_thread)));
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
            let rng_seed = Hash::from(&rng_seed_bits.into_vec()).to_bytes().to_vec();

            (cum_sum, rng_seed)
        } else {
            // special case: lookback before cycle 0

            // get initial rolls
            let mut cum_sum: Vec<(u64, Address)> = Vec::new(); // amount, thread, address
            let mut cum_sum_cursor = 0u64;
            for scan_thread in 0..self.cfg.thread_count {
                let init_rolls = &self.initial_rolls.as_ref().ok_or_else( ||
                    ConsensusError::PosCycleUnavailable(format!(
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
            .ok_or_else(|| ConsensusError::ContainerInconsistency("draw cum_sum is empty".into()))?
            .0;

        // init RNG
        let mut rng = Xoshiro256PlusPlus::from_seed(rng_seed.try_into().map_err(|_| {
            ConsensusError::ContainerInconsistency("could not seed RNG with computed seed".into())
        })?);

        // perform draws
        let distribution = Uniform::new(0, cum_sum_max);
        let mut draws: HashMap<Slot, (Address, Vec<Address>)> =
            HashMap::with_capacity(blocks_in_cycle);
        let cycle_first_period = cycle * self.cfg.periods_per_cycle;
        let cycle_last_period = (cycle + 1) * self.cfg.periods_per_cycle - 1;
        if cycle_first_period == 0 {
            // genesis slots: force block creator and endorsement creator address draw
            let genesis_addr = Address::from_public_key(&derive_public_key(&self.cfg.genesis_key))?;
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

    pub fn draw_endorsement_producers(
        &mut self,
        slot: Slot,
    ) -> Result<Vec<Address>, ConsensusError> {
        Ok(self.draw(slot)?.1)
    }

    pub fn draw_block_producer(&mut self, slot: Slot) -> Result<Address, ConsensusError> {
        Ok(self.draw(slot)?.0)
    }

    /// returns (block producers, vec<endorsement producers>)
    fn draw(&mut self, slot: Slot) -> Result<(Address, Vec<Address>), ConsensusError> {
        let cycle = slot.get_cycle(self.cfg.periods_per_cycle);
        let cycle_draws = self.get_cycle_draws(cycle)?;
        Ok(cycle_draws
            .get(&slot)
            .ok_or_else(|| {
                ConsensusError::ContainerInconsistency(format!(
                    "draw cycle computed for cycle {} but slot {} absent",
                    cycle, slot
                ))
            })?
            .clone())
    }

    /// Update internal states after a set of blocks become final
    /// see /consensus/pos.md#when-a-block-b-in-thread-tau-and-cycle-n-becomes-final
    pub fn note_final_blocks(
        &mut self,
        blocks: BlockHashMap<&ActiveBlock>,
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
        addrs: &AddressHashSet,
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
                        ok_nok_counts: AddressHashMap::default(),
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
    pub fn get_roll_sell_credit(
        &self,
        cycle: u64,
        thread: u8,
    ) -> Result<AddressHashMap<Amount>, ConsensusError> {
        let mut res = AddressHashMap::default();
        if let Some(target_cycle) =
            cycle.checked_sub(self.cfg.pos_lookback_cycles + self.cfg.pos_lock_cycles + 1)
        {
            let roll_data = self
                .get_final_roll_data(target_cycle, thread)
                .ok_or(ConsensusError::NotFinalRollError)?;
            if !roll_data.is_complete(self.cfg.periods_per_cycle) {
                return Err(ConsensusError::NotFinalRollError); // target_cycle not completely final
            }
            for (addr, update) in roll_data.cycle_updates.0.iter() {
                let sale_delta = update.roll_sales.saturating_sub(update.roll_purchases);
                if sale_delta > 0 {
                    res.insert(
                        *addr,
                        self.cfg
                            .roll_price
                            .checked_mul_u64(sale_delta)
                            .ok_or(ConsensusError::RollOverflowError)?,
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
    ) -> Result<AddressHashSet, ConsensusError> {
        // compute target cycle
        if cycle <= self.cfg.pos_lookback_cycles {
            // no lookback cycles yet: do not deactivate anyone
            return Ok(AddressHashSet::default());
        }
        let target_cycle = cycle - self.cfg.pos_lookback_cycles - 1;

        // get roll data from all threads for addresses belonging to address_thread
        let mut addr_stats: AddressHashMap<(u64, u64)> = AddressHashMap::default();
        for thread in 0..self.cfg.thread_count {
            // get roll data
            let roll_data = self
                .get_final_roll_data(target_cycle, thread)
                .ok_or(ConsensusError::NotFinalRollError)?;
            if !roll_data.is_complete(self.cfg.periods_per_cycle) {
                return Err(ConsensusError::NotFinalRollError); // target_cycle not completely final
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
        let res: AddressHashSet = addr_stats
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
        addrs: &AddressHashSet,
    ) -> AddressHashMap<u64> {
        if cycle < 1 + self.cfg.pos_lookback_cycles {
            return AddressHashMap::default();
        }
        let start_cycle = cycle
            .saturating_sub(self.cfg.pos_lookback_cycles)
            .saturating_sub(self.cfg.pos_lock_cycles);
        let end_cycle = cycle - self.cfg.pos_lookback_cycles - 1;
        let mut res: AddressHashMap<u64> = AddressHashMap::default();
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
    pub fn get_lookback_roll_count(
        &self,
        source_cycle: u64,
        thread: u8,
    ) -> Result<&RollCounts, ConsensusError> {
        if source_cycle > self.cfg.pos_lookback_cycles {
            // nominal case: lookback after or at cycle 0
            let target_cycle = source_cycle - self.cfg.pos_lookback_cycles - 1;
            if let Some(state) = self.get_final_roll_data(target_cycle, thread) {
                if !state.is_complete(self.cfg.periods_per_cycle) {
                    return Err(ConsensusError::PosCycleUnavailable(
                        "target cycle incomplete".to_string(),
                    ));
                }
                Ok(&state.roll_count)
            } else {
                Err(ConsensusError::PosCycleUnavailable(
                    "target cycle unavailable".to_string(),
                ))
            }
        } else if let Some(init) = &self.initial_rolls {
            Ok(&init[thread as usize])
        } else {
            Err(ConsensusError::PosCycleUnavailable(
                "negative cycle unavailable".to_string(),
            ))
        }
    }
}
