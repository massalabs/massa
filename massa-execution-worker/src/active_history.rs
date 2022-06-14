use massa_execution_exports::ExecutionOutput;
use massa_hash::Hash;
use massa_ledger_exports::{
    LedgerEntry, LedgerEntryUpdate, SetOrDelete, SetOrKeep, SetUpdateOrDelete,
};
use massa_models::{Address, Amount};
use std::collections::VecDeque;

#[derive(Default)]
pub(crate) struct ActiveHistory(pub VecDeque<ExecutionOutput>);

pub enum HistorySearchResult<T> {
    Found(T),
    NotFound,
    Deleted,
}

impl ActiveHistory {
    /// Lazily query (from end to beginning) the active balance of an address at a given slot.
    ///
    /// Returns None if the address balance could not be determined from the active history.
    pub fn fetch_active_history_balance(
        &self,
        addr: &Address,
        index: Option<usize>,
    ) -> HistorySearchResult<Amount> {
        let iter = self.0.iter().skip(index.unwrap_or_default()).rev();

        for output in iter {
            match output.state_changes.ledger_changes.0.get(addr) {
                Some(SetUpdateOrDelete::Set(v)) => {
                    return HistorySearchResult::Found(v.parallel_balance)
                }
                Some(SetUpdateOrDelete::Update(LedgerEntryUpdate {
                    parallel_balance: SetOrKeep::Set(v),
                    ..
                })) => return HistorySearchResult::Found(*v),
                Some(SetUpdateOrDelete::Delete) => return HistorySearchResult::Deleted,
                _ => (),
            }
        }
        HistorySearchResult::NotFound
    }

    /// Lazily query (from end to beginning) the active balance of an address at a given slot.
    ///
    /// Returns None if the address balance could not be determined from the active history.
    pub fn fetch_active_history_bytecode(
        &self,
        addr: &Address,
        index: Option<usize>,
    ) -> HistorySearchResult<Vec<u8>> {
        let iter = self.0.iter().skip(index.unwrap_or_default()).rev();

        for output in iter {
            match output.state_changes.ledger_changes.0.get(addr) {
                Some(SetUpdateOrDelete::Set(v)) => {
                    return HistorySearchResult::Found(v.bytecode.to_vec())
                }
                Some(SetUpdateOrDelete::Update(LedgerEntryUpdate {
                    bytecode: SetOrKeep::Set(v),
                    ..
                })) => return HistorySearchResult::Found(v.to_vec()),
                Some(SetUpdateOrDelete::Delete) => return HistorySearchResult::Deleted,
                _ => (),
            }
        }
        HistorySearchResult::NotFound
    }

    /// Lazily query (from end to beginning) the active datastore entry of an address at a given slot.
    ///
    /// Returns None if the datastore entry could not be determined from the active history.
    pub fn fetch_active_history_data_entry(
        &self,
        addr: &Address,
        key: &Hash,
        index: Option<usize>,
    ) -> HistorySearchResult<Vec<u8>> {
        let iter = self.0.iter().skip(index.unwrap_or_default()).rev();

        for output in iter {
            match output.state_changes.ledger_changes.0.get(addr) {
                Some(SetUpdateOrDelete::Set(LedgerEntry { datastore, .. })) => {
                    match datastore.get(key) {
                        Some(value) => return HistorySearchResult::Found(value.to_vec()),
                        None => (),
                    }
                }
                Some(SetUpdateOrDelete::Update(LedgerEntryUpdate { datastore, .. })) => {
                    match datastore.get(key) {
                        Some(SetOrDelete::Set(value)) => {
                            return HistorySearchResult::Found(value.to_vec())
                        }
                        Some(SetOrDelete::Delete) => return HistorySearchResult::Deleted,
                        None => (),
                    }
                }
                Some(SetUpdateOrDelete::Delete) => return HistorySearchResult::Deleted,
                None => (),
            }
        }
        HistorySearchResult::NotFound
    }
}
