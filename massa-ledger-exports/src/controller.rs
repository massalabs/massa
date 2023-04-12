use massa_hash::Hash;
use massa_models::{
    address::Address, amount::Amount, bytecode::Bytecode, error::ModelsError, slot::Slot,
    streaming_step::StreamingStep,
};
use std::collections::BTreeSet;
use std::fmt::Debug;

use crate::{Key, LedgerBatch, LedgerChanges, LedgerError};

pub trait LedgerController: Send + Sync + Debug {
    /// Allows applying `LedgerChanges` to the final ledger
    /// * final_state_data should be non-None only if we are storing a final_state snapshot.
    fn apply_changes(&mut self, changes: LedgerChanges, slot: Slot);

    /// Loads ledger from file
    fn load_initial_ledger(&mut self) -> Result<(), LedgerError>;

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

    /// Get the current disk ledger hash
    fn get_ledger_hash(&self) -> Hash;

    /// Get a part of the ledger
    /// Used for bootstrap
    /// Return: Tuple with data and last key
    fn get_ledger_part(
        &self,
        last_key: StreamingStep<Key>,
    ) -> Result<(Vec<u8>, StreamingStep<Key>), ModelsError>;

    /// Set a part of the ledger
    /// Used for bootstrap
    /// Return: Last key inserted
    fn set_ledger_part(&self, data: Vec<u8>) -> Result<StreamingStep<Key>, ModelsError>;

    /// Reset the ledger
    ///
    /// USED FOR BOOTSTRAP ONLY
    fn reset(&mut self);

    fn set_initial_slot(&mut self, slot: Slot);

    fn get_slot(&self) -> Result<Slot, ModelsError>;

    fn set_final_state_hash(&mut self, data: Vec<u8>);

    fn get_final_state(&self) -> Result<Vec<u8>, ModelsError>;

    fn backup_db(&self, slot: Slot);

    fn apply_changes_to_batch(
        &mut self,
        changes: LedgerChanges,
        slot: Slot,
        ledger_batch: &mut LedgerBatch,
    );

    fn write_batch(&mut self, batch: LedgerBatch);

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
