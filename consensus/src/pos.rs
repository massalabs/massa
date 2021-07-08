use crate::{block_graph::ActiveBlock, ConsensusConfig, ConsensusError};
use bitvec::prelude::*;
use models::{Address, Block, BlockId, Operation, Slot};

use crypto::hash::Hash;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap, VecDeque};

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

struct FinalRollThreadData {
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

struct ProofOfStake {
    /// Config
    cfg: ConsensusConfig,
    /// Index by thread and cycle number
    final_roll_data: Vec<VecDeque<FinalRollThreadData>>,
}

#[derive(Debug, Serialize, Deserialize)]
struct ExportProofOfStake {
    /// Index by thread and cycle number
    final_roll_data: Vec<Vec<ExportFinalRollThreadData>>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
struct ExportFinalRollThreadData {
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
    pub fn new() -> ProofOfStake {
        todo!()
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

    pub fn from_export(cfg: ConsensusConfig, export: ExportProofOfStake) -> ProofOfStake {
        ProofOfStake {
            cfg,
            final_roll_data: export
                .final_roll_data
                .into_iter()
                .map(|vec| {
                    vec.into_iter()
                        .map(|frtd| FinalRollThreadData::from_export(frtd))
                        .collect::<VecDeque<FinalRollThreadData>>()
                })
                .collect(),
        }
    }

    pub fn draw(&self, slot: Slot) -> Result<Address, ConsensusError> {
        // the current random_selector.rs should be fused inside, by adding a draw(slot) -> Result<Address, Error> method to the ProofOfStake struct. Add an internal cache.
        todo!()
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
                        (self.cfg.pos_lookback_cycles + self.cfg.pos_lock_cycles + 1) as usize,
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
                    entry
                        .rng_seed
                        .push(Hash::hash(&block_id.to_bytes()).to_bytes()[0] >> 7 == 1);
                } else {
                    // we are applying a miss
                    // append the 1st bit of the hash of the slot of the miss to the RNG seed bitfield
                    entry
                        .rng_seed
                        .push(Hash::hash(&slot.to_bytes_key()).to_bytes()[0] >> 7 == 1);
                }
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
