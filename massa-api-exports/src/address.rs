// Copyright (c) 2022 MASSA LABS <info@massa.net>

use massa_models::address::ExecutionAddressCycleInfo;
use massa_models::endorsement::EndorsementId;
use massa_models::operation::OperationId;
use massa_models::slot::{IndexedSlot, Slot};
use massa_models::{address::Address, amount::Amount, block_id::BlockId};
use serde::{Deserialize, Serialize};

use crate::slot::SlotAmount;

/// All you ever dream to know about an address
#[derive(Debug, Deserialize, Serialize)]
pub(crate)  struct AddressInfo {
    /// the address
    pub(crate)  address: Address,
    /// the thread the address belongs to
    pub(crate)  thread: u8,

    /// final balance
    pub(crate)  final_balance: Amount,
    /// final roll count
    pub(crate)  final_roll_count: u64,
    /// final datastore keys
    pub(crate)  final_datastore_keys: Vec<Vec<u8>>,

    /// candidate balance
    pub(crate)  candidate_balance: Amount,
    /// candidate roll count
    pub(crate)  candidate_roll_count: u64,
    /// candidate datastore keys
    pub(crate)  candidate_datastore_keys: Vec<Vec<u8>>,

    /// deferred credits
    pub(crate)  deferred_credits: Vec<SlotAmount>,

    /// next block draws
    pub(crate)  next_block_draws: Vec<Slot>,
    /// next endorsement draws
    pub(crate)  next_endorsement_draws: Vec<IndexedSlot>,

    /// created blocks
    pub(crate)  created_blocks: Vec<BlockId>,
    /// created operations
    pub(crate)  created_operations: Vec<OperationId>,
    /// created endorsements
    pub(crate)  created_endorsements: Vec<EndorsementId>,

    /// cycle information
    pub(crate)  cycle_infos: Vec<ExecutionAddressCycleInfo>,
}

impl std::fmt::Display for AddressInfo {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        writeln!(f, "Address {} (thread {}):", self.address, self.thread)?;
        writeln!(
            f,
            "\tBalance: final={}, candidate={}",
            self.final_balance, self.candidate_balance
        )?;
        writeln!(
            f,
            "\tRolls: final={}, candidate={}",
            self.final_roll_count, self.candidate_roll_count
        )?;
        write!(f, "\tLocked coins:")?;
        if self.deferred_credits.is_empty() {
            writeln!(f, "0")?;
        } else {
            for slot_amount in &self.deferred_credits {
                writeln!(
                    f,
                    "\t\t{} locked coins will be unlocked at slot {}",
                    slot_amount.amount, slot_amount.slot
                )?;
            }
        }
        writeln!(f, "\tCycle infos:")?;
        for cycle_info in &self.cycle_infos {
            writeln!(
                f,
                "\t\tCycle {} ({}): produced {} and missed {} blocks{}",
                cycle_info.cycle,
                if cycle_info.is_final {
                    "final"
                } else {
                    "candidate"
                },
                cycle_info.ok_count,
                cycle_info.nok_count,
                match cycle_info.active_rolls {
                    Some(rolls) => format!(" with {} active rolls", rolls),
                    None => "".into(),
                },
            )?;
        }
        //writeln!(f, "\tProduced blocks: {}", self.created_blocks.iter().map(|id| id.to_string()).intersperse(", ".into()).collect())?;
        //writeln!(f, "\tProduced operations: {}", self.created_operations.iter().map(|id| id.to_string()).intersperse(", ".into()).collect())?;
        //writeln!(f, "\tProduced endorsements: {}", self.created_endorsements.iter().map(|id| id.to_string()).intersperse(", ".into()).collect())?;
        Ok(())
    }
}

impl AddressInfo {
    /// Only essential info about an address
    pub(crate)  fn compact(&self) -> CompactAddressInfo {
        CompactAddressInfo {
            address: self.address,
            thread: self.thread,
            active_rolls: self
                .cycle_infos
                .last()
                .and_then(|c| c.active_rolls)
                .unwrap_or_default(),
            final_rolls: self.final_roll_count,
            candidate_rolls: self.candidate_roll_count,
            final_balance: self.final_balance,
            candidate_balance: self.candidate_balance,
        }
    }
}

/// Less information about an address
#[derive(Debug, Serialize, Deserialize)]
pub(crate)  struct CompactAddressInfo {
    /// the address
    pub(crate)  address: Address,
    /// the thread it is
    pub(crate)  thread: u8,
    /// candidate rolls
    pub(crate)  candidate_rolls: u64,
    /// final rolls
    pub(crate)  final_rolls: u64,
    /// active rolls
    pub(crate)  active_rolls: u64,
    /// final balance
    pub(crate)  final_balance: Amount,
    /// candidate balance
    pub(crate)  candidate_balance: Amount,
}

impl std::fmt::Display for CompactAddressInfo {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        writeln!(f, "Address: {} (thread {}):", self.address, self.thread)?;
        writeln!(
            f,
            "\tBalance: final={}, candidate={}",
            self.final_balance, self.candidate_balance
        )?;
        writeln!(
            f,
            "\tRolls: active={}, final={}, candidate={}",
            self.active_rolls, self.final_rolls, self.candidate_rolls
        )?;
        Ok(())
    }
}
