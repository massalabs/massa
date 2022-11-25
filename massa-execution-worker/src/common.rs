use massa_ledger_exports::{LedgerController, SetOrDelete, SetUpdateOrDelete};
use massa_models::address::Address;

use std::collections::{BTreeSet, VecDeque};
use massa_execution_exports::ExecutionOutput;

/// Get every final and active datastore key of the given address
pub fn get_final_and_candidate_datastore_keys(
    ledger: &Box<dyn LedgerController>,
    active_history_0: &VecDeque<ExecutionOutput>,
    addr: &Address,
) -> (BTreeSet<Vec<u8>>, BTreeSet<Vec<u8>>) {
    // here, get the final keys from the final ledger, and make a copy of it for the candidate list
    // let final_keys = final_state.read().ledger.get_datastore_keys(addr);
    let final_keys = ledger.get_datastore_keys(addr);
    let mut candidate_keys = final_keys.clone();

    // here, traverse the history from oldest to newest, applying additions and deletions
    // for output in &active_history.read().0 {
    for output in active_history_0 {
        match output.state_changes.ledger_changes.get(addr) {
            // address absent from the changes
            None => (),

            // address ledger entry being reset to an absolute new list of keys
            Some(SetUpdateOrDelete::Set(new_ledger_entry)) => {
                candidate_keys = new_ledger_entry.datastore.keys().cloned().collect();
            }

            // address ledger entry being updated
            Some(SetUpdateOrDelete::Update(entry_updates)) => {
                for (ds_key, ds_update) in &entry_updates.datastore {
                    match ds_update {
                        SetOrDelete::Set(_) => candidate_keys.insert(ds_key.clone()),
                        SetOrDelete::Delete => candidate_keys.remove(ds_key),
                    };
                }
            }

            // address ledger entry being deleted
            Some(SetUpdateOrDelete::Delete) => {
                candidate_keys.clear();
            }
        }
    }

    (final_keys, candidate_keys)
}
