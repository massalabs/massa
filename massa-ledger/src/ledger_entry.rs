use crate::ledger_changes::LedgerEntryUpdate;
use crate::types::{Applicable, SetOrDelete};
use massa_hash::hash::Hash;
use massa_models::Amount;
use std::collections::BTreeMap;

/// structure defining a ledger entry
#[derive(Default, Debug, Clone)]
pub struct LedgerEntry {
    pub parallel_balance: Amount,
    pub bytecode: Vec<u8>,
    pub datastore: BTreeMap<Hash, Vec<u8>>,
}

/// LedgerEntryUpdate can be applied to a LedgerEntry
impl Applicable<LedgerEntryUpdate> for LedgerEntry {
    /// applies a LedgerEntryUpdate
    fn apply(&mut self, update: LedgerEntryUpdate) {
        update.parallel_balance.apply_to(&mut self.parallel_balance);
        update.bytecode.apply_to(&mut self.bytecode);
        for (key, value_update) in update.datastore {
            match value_update {
                SetOrDelete::Set(v) => {
                    self.datastore.insert(key, v);
                }
                SetOrDelete::Delete => {
                    self.datastore.remove(&key);
                }
            }
        }
    }
}
