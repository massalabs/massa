use massa_hash::Hash;
use massa_models::{Address, Amount, ModelsError, Slot};
use std::collections::BTreeMap;
use std::fmt::Debug;

use crate::LedgerChanges;

pub trait LedgerController: Send + Sync + Debug {
    /// Allows applying `LedgerChanges` to the final ledger
    fn apply_changes(&mut self, changes: LedgerChanges, slot: Slot);

    /// Gets the sequential balance of a ledger entry
    ///
    /// # Returns
    /// The sequential balance, or None if the ledger entry was not found
    fn get_sequential_balance(&self, addr: &Address) -> Option<Amount>;

    /// Gets the parallel balance of a ledger entry
    ///
    /// # Returns
    /// The parallel balance, or None if the ledger entry was not found
    fn get_parallel_balance(&self, addr: &Address) -> Option<Amount>;

    /// Gets a copy of the bytecode of a ledger entry
    ///
    /// # Returns
    /// A copy of the found bytecode, or None if the ledger entry was not found
    fn get_bytecode(&self, addr: &Address) -> Option<Vec<u8>>;

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
    fn get_data_entry(&self, addr: &Address, key: &Hash) -> Option<Vec<u8>>;

    /// Checks for the existence of a datastore entry for a given address.
    ///
    /// # Arguments
    /// * `addr`: target address
    /// * `key`: datastore key
    ///
    /// # Returns
    /// true if the datastore entry was found, or false if the ledger entry or datastore entry was not found
    fn has_data_entry(&self, addr: &Address, key: &Hash) -> bool;

    /// # Returns
    /// A copy of the datastore sorted by key
    fn get_entire_datastore(&self, addr: &Address) -> BTreeMap<Hash, Vec<u8>>;

    /// Get a part of the ledger
    /// Used for bootstrap
    /// Return: Tuple with data and last key
    fn get_ledger_part(
        &self,
        last_key: &Option<Vec<u8>>,
    ) -> Result<(Vec<u8>, Option<Vec<u8>>), ModelsError>;

    /// Set a part of the ledger
    /// Used for bootstrap
    /// Return: Last key inserted
    fn set_ledger_part(&self, data: Vec<u8>) -> Result<Option<Vec<u8>>, ModelsError>;

    /// Get every address and their corresponding balance.
    ///
    /// IMPORTANT: This should only be used for debug and test purposes.
    ///
    /// # Returns
    /// A BTreeMap with the address as key and the balance as value
    #[cfg(feature = "testing")]
    fn get_every_address(&self) -> BTreeMap<Address, Amount>;
}
