use bitvec::prelude::*;
use models::{Address, Slot};
use std::collections::{BTreeMap, HashMap, VecDeque};

use serde::{Deserialize, Serialize};

use crate::{block_graph::ActiveBlock, ConsensusError};

struct FinalRollThreadData {
    /// Cycle number
    cycle: u64,
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
    /// Cycle indexed by thread
    last_final_block_cycle: Vec<u64>,
    /// Index by thread and cycle number
    final_roll_data: Vec<VecDeque<FinalRollThreadData>>,
}

#[derive(Debug, Serialize, Deserialize)]
struct ExportProofOfStake {
    /// Cycle indexed by thread
    last_final_block_cycle: Vec<u64>,
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
            last_final_block_cycle: self.last_final_block_cycle.clone(),
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

    pub fn from_export(export: ExportProofOfStake) -> ProofOfStake {
        ProofOfStake {
            last_final_block_cycle: export.last_final_block_cycle.clone(),
            final_roll_data: export
                .final_roll_data
                .iter()
                .map(|vec| {
                    vec.into_iter()
                        .map(|frtd| FinalRollThreadData::from_export(frtd.clone()))
                        .collect::<VecDeque<FinalRollThreadData>>()
                })
                .collect(),
        }
    }

    pub fn draw(&self, slot: Slot) -> Result<Address, ConsensusError> {
        // the current random_selector.rs should be fused inside, by adding a draw(slot) -> Result<Address, Error> method to the ProofOfStake struct. Add an internal cache.
        todo!()
    }

    pub fn note_final_block(
        &self,
        misses: Vec<Slot>,
        a_block: &ActiveBlock,
    ) -> Result<(), ConsensusError> {
        todo!()
        //update internal states after a block becomes final. When multiple blocks become final,
        // this method should be called in slot order.
        // "misses" is the list of misses in the same thread between the block and its parent in the same thread.

        // note_final_block: When a block B in thread Tau and cycle N becomes final
        //
        // if N > last_final_block_cycle[Tau]:
        //
        // update last_final_block_cycle[Tau]
        // pop front for final_roll_data[thread] until the 1st element represents cycle N-4
        // push back a new last element in final_roll_data that represents cycle N:
        //
        // inherit FinalRollThreadData.roll_count from cycle N-1
        // empty FinalRollThreadData.cycle_purchases, FinalRollThreadData.cycle_sales, FinalRollThreadData.rng_seed
        //
        //
        //
        //
        // if there were misses between B and its parent, for each of them in order:
        //
        // push the 1st bit of Sha256( miss.slot.to_bytes_key() ) in final_roll_data[thread].rng_seed
        //
        //
        // push the 1st bit of BlockId in final_roll_data[thread].rng_seed
        // overwrite the FinalRollThreadData sales/purchase entries at cycle N with the ones from the ActiveBlock
    }

    pub fn get_last_final_block_cycle(&self, thread: u8) -> u64 {
        // returns the cycle of the last final block in thread
        self.last_final_block_cycle[thread as usize]
    }

    pub fn get_final_roll_data(&self, cycle: u64, thread: u8) -> Option<&FinalRollThreadData> {
        todo!()
        // that returns None if that cycle is not stored
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
            roll_count: export.roll_count.iter().cloned().collect(),
            cycle_purchases: export.cycle_purchases.iter().cloned().collect(),
            cycle_sales: export.cycle_sales.iter().cloned().collect(),
            rng_seed: export.rng_seed.clone(),
        }
    }
}
