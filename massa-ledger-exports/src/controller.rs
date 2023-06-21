use massa_models::{address::Address, amount::Amount, bytecode::Bytecode};
use std::collections::BTreeSet;
use std::fmt::Debug;

use crate::{LedgerChanges, LedgerError};
use massa_db_exports::DBBatch;

pub trait LedgerController: Send + Sync + Debug {
    /// Loads ledger from file
    fn load_initial_ledger(&mut self, only_use_xor: bool) -> Result<(), LedgerError>;

    /// Gets the balance of a ledger entry
    ///
    /// # Returns
    /// The balance, or None if the ledger entry was not found
    fn get_balance(&self, addr: &Address) -> Option<Amount>;

    /// Gets a copy of the bytecode of a ledger entry
    ///
    /// # Returns
    /// A copy of the found bytecode, or None if the ledger entry was not found
    fn get_bytecode(&self, addr: &Address) -> Option<Bytecode>;

    /// Checks if a ledger entry exists
    ///
    /// # Returns
    /// true if it exists, false otherwise.
    fn entry_exists(&self, addr: &Address) -> bool;

    /// Gets a copy of the value of a datastore entry for a given address.
    ///
    /// # Arguments
    /// * `addr`: target address
    /// * `key`: datastore key
    ///
    /// # Returns
    /// A copy of the datastore value, or `None` if the ledger entry or datastore entry was not found
    fn get_data_entry(&self, addr: &Address, key: &[u8]) -> Option<Vec<u8>>;

    /// Get every key of the datastore for a given address.
    ///
    /// # Returns
    /// A `BTreeSet` of the datastore keys
    fn get_datastore_keys(&self, addr: &Address) -> Option<BTreeSet<Vec<u8>>>;

    /// Reset the ledger
    ///
    /// USED FOR BOOTSTRAP ONLY
    fn reset(&mut self, only_use_xor: bool);

    fn apply_changes_to_batch(&mut self, changes: LedgerChanges, ledger_batch: &mut DBBatch);

    /// Deserializes the key and value, useful after bootstrap
    fn is_key_value_valid(&self, serialized_key: &[u8], serialized_value: &[u8]) -> bool;

    /// Get every address and their corresponding balance.
    ///
    /// IMPORTANT: This should only be used for debug and test purposes.
    ///
    /// # Returns
    /// A `BTreeMap` with the address as key and the balance as value
    #[cfg(feature = "testing")]
    fn get_every_address(&self) -> std::collections::BTreeMap<Address, Amount>;

    /// Get the entire datastore for a given address.
    ///
    /// IMPORTANT: This should only be used for debug purposes.
    ///
    /// # Returns
    /// A `BTreeMap` with the entry hash as key and the data bytes as value
    #[cfg(feature = "testing")]
    fn get_entire_datastore(&self, addr: &Address) -> std::collections::BTreeMap<Vec<u8>, Vec<u8>>;
}
