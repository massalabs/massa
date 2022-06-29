use massa_execution_exports::ExecutionOutput;
use massa_hash::Hash;
use massa_ledger_exports::{
    LedgerEntry, LedgerEntryUpdate, SetOrDelete, SetOrKeep, SetUpdateOrDelete,
};
use massa_models::{prehash::Map, Address, Amount, Slot};
use massa_pos_exports::ProductionStats;
use std::collections::{BTreeMap, VecDeque};

#[derive(Default)]
/// History of the outputs of recently executed slots.
/// Slots should be consecutive, oldest at the beginning and latest at the back.
pub(crate) struct ActiveHistory(pub VecDeque<ExecutionOutput>);

/// Result of a lazy, active history search
pub enum HistorySearchResult<T> {
    Present(T),
    Absent,
    NoInfo,
}

impl ActiveHistory {
    /// Lazily query (from end to beginning) the active balance of an address after a given index.
    ///
    /// Returns a `HistorySearchResult`.
    pub fn fetch_active_history_balance(&self, addr: &Address) -> HistorySearchResult<Amount> {
        for output in self.0.iter().rev() {
            match output.state_changes.ledger_changes.0.get(addr) {
                Some(SetUpdateOrDelete::Set(v)) => {
                    return HistorySearchResult::Present(v.parallel_balance)
                }
                Some(SetUpdateOrDelete::Update(LedgerEntryUpdate {
                    parallel_balance: SetOrKeep::Set(v),
                    ..
                })) => return HistorySearchResult::Present(*v),
                Some(SetUpdateOrDelete::Delete) => return HistorySearchResult::Absent,
                _ => (),
            }
        }
        HistorySearchResult::NoInfo
    }

    /// Lazily query (from end to beginning) the active bytecode of an address after a given index.
    ///
    /// Returns a `HistorySearchResult`.
    pub fn fetch_active_history_bytecode(&self, addr: &Address) -> HistorySearchResult<Vec<u8>> {
        for output in self.0.iter().rev() {
            match output.state_changes.ledger_changes.0.get(addr) {
                Some(SetUpdateOrDelete::Set(v)) => {
                    return HistorySearchResult::Present(v.bytecode.to_vec())
                }
                Some(SetUpdateOrDelete::Update(LedgerEntryUpdate {
                    bytecode: SetOrKeep::Set(v),
                    ..
                })) => return HistorySearchResult::Present(v.to_vec()),
                Some(SetUpdateOrDelete::Delete) => return HistorySearchResult::Absent,
                _ => (),
            }
        }
        HistorySearchResult::NoInfo
    }

    /// Lazily query (from end to beginning) the active datastore entry of an address after a given index.
    ///
    /// Returns a `HistorySearchResult`.
    pub fn fetch_active_history_data_entry(
        &self,
        addr: &Address,
        key: &Hash,
    ) -> HistorySearchResult<Vec<u8>> {
        for output in self.0.iter().rev() {
            match output.state_changes.ledger_changes.0.get(addr) {
                Some(SetUpdateOrDelete::Set(LedgerEntry { datastore, .. })) => {
                    match datastore.get(key) {
                        Some(value) => return HistorySearchResult::Present(value.to_vec()),
                        None => return HistorySearchResult::Absent,
                    }
                }
                Some(SetUpdateOrDelete::Update(LedgerEntryUpdate { datastore, .. })) => {
                    match datastore.get(key) {
                        Some(SetOrDelete::Set(value)) => {
                            return HistorySearchResult::Present(value.to_vec())
                        }
                        Some(SetOrDelete::Delete) => return HistorySearchResult::Absent,
                        None => (),
                    }
                }
                Some(SetUpdateOrDelete::Delete) => return HistorySearchResult::Absent,
                None => (),
            }
        }
        HistorySearchResult::NoInfo
    }

    /// Starting from the newest element in history, return the first existing roll change of `addr`.
    ///
    /// # Arguments
    /// * `addr`: address to fetch the rolls from
    pub fn fetch_roll_count(&self, addr: &Address) -> Option<u64> {
        self.0.iter().rev().find_map(|output| {
            output
                .state_changes
                .roll_state_changes
                .roll_changes
                .get(addr)
                .cloned()
        })
    }

    /// Traverse the whole history and return every deferred credit of `addr` _after_ `slot` (included).
    ///
    /// # Arguments
    /// * `slot`: slot _after_ which we fetch the credits
    /// * `addr`: address to fetch the credits from
    #[allow(dead_code)]
    pub fn fetch_deferred_credits_after(
        &self,
        slot: &Slot,
        addr: &Address,
    ) -> BTreeMap<Slot, Amount> {
        self.0
            .iter()
            .flat_map(|output| {
                output
                    .state_changes
                    .roll_state_changes
                    .deferred_credits
                    .range(slot..)
                    .filter_map(|(&slot, credits)| credits.get(addr).map(|&amount| (slot, amount)))
            })
            .collect()
    }

    /// Traverse the whole history and return every deferred credit _at_ `slot`
    ///
    /// # Arguments
    /// * `slot`: slot _at_ which we fetch the credits
    pub fn fetch_all_deferred_credits_at(&self, slot: &Slot) -> Map<Address, Amount> {
        self.0
            .iter()
            .filter_map(|output| {
                output
                    .state_changes
                    .roll_state_changes
                    .deferred_credits
                    .get(slot)
                    .cloned()
            })
            .flatten()
            .collect()
    }

    /// Retrieve the production stats of `addr` as they are in the last element of the history.
    ///
    /// # Arguments
    /// * `addr`:  address to fetch the production stats from
    #[allow(dead_code)]
    pub fn fetch_production_stats(&self, addr: &Address) -> Option<ProductionStats> {
        self.0
            .back()
            .map(|output| {
                output
                    .state_changes
                    .roll_state_changes
                    .production_stats
                    .get(addr)
                    .cloned()
            })
            .flatten()
    }
}
