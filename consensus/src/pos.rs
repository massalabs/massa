use crate::{block_graph::ActiveBlock, ConsensusConfig, ConsensusError};
use bitvec::prelude::*;
use std::collections::{BTreeMap, HashMap, VecDeque};

use crypto::hash::Hash;
use models::{Address, BlockId, Operation, Slot};
use serde::{Deserialize, Serialize};

pub trait OperationPosInterface {
    /// returns [thread][roll_involved_addr](compensated_bought_rolls, compensated_sold_rolls)
    fn get_roll_changes(
        &self,
        thread_count: u8,
    ) -> Result<Vec<HashMap<Address, (u64, u64)>>, ConsensusError>;
}

impl OperationPosInterface for Operation {
    fn get_roll_changes(
        &self,
        thread_count: u8,
    ) -> Result<Vec<HashMap<Address, (u64, u64)>>, ConsensusError> {
        todo!()
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
    /// Initial rolls: we keep them as long as negative cycle draws are needed
    initial_rolls: Vec<Option<BTreeMap<Address, u64>>>,
    // Initial seeds: they are lightweight, we always keep them
    // the seed for cycle -N is obtained by hashing N times the value ConsensusConfig.initial_draw_seed
    // the seeds are indexed from -1 to -N
    initial_seeds: Vec<Vec<u8>>,
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
    pub async fn new(cfg: ConsensusConfig) -> Result<ProofOfStake, ConsensusError> {
        let mut final_roll_data = Vec::with_capacity(cfg.thread_count as usize);
        let mut initial_rolls: Vec<Option<BTreeMap<Address, u64>>> =
            Vec::with_capacity(cfg.thread_count as usize);
        let initial_roll_count = ProofOfStake::get_initial_rolls(&cfg).await?;
        for (thread, thread_rolls) in initial_roll_count.into_iter().enumerate() {
            // initial rolls
            initial_rolls.push(Some(thread_rolls.clone()));
            // init thread history with one cycle
            // note that the genesis blocks are not taken into account in the rng_seed
            let mut history = VecDeque::with_capacity(
                (cfg.pos_lock_cycles + cfg.pos_lock_cycles + 2 + 1) as usize,
            );
            let frtd = FinalRollThreadData {
                cycle: 0,
                last_final_slot: Slot::new(0, thread as u8),
                roll_count: thread_rolls,
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
            initial_rolls,
            initial_seeds: ProofOfStake::generate_initial_seeds(&cfg),
            cfg,
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
        let needed_initial_rolls: Vec<bool> = final_roll_data
            .iter()
            .map(|v| v[0].cycle < cfg.pos_lock_cycles + cfg.pos_lock_cycles + 1)
            .collect();
        let mut initial_rolls = vec![None; cfg.thread_count as usize];
        if needed_initial_rolls.iter().any(|v| *v) {
            let initial_roll_count = ProofOfStake::get_initial_rolls(&cfg).await?;
            for (thread, rolls) in initial_roll_count.into_iter().enumerate() {
                if needed_initial_rolls[thread as usize] {
                    initial_rolls[thread as usize] = Some(rolls);
                }
            }
        }

        // generate object
        Ok(ProofOfStake {
            final_roll_data,
            initial_rolls,
            initial_seeds: ProofOfStake::generate_initial_seeds(&cfg),
            cfg,
        })
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

        // if initial rolls are not needed in some threads, remove them to free memory
        for (thread, final_rolls) in self.final_roll_data.iter().enumerate() {
            if final_rolls[0].cycle >= self.cfg.pos_lock_cycles + self.cfg.pos_lock_cycles + 1 {
                self.initial_rolls[thread] = None;
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
