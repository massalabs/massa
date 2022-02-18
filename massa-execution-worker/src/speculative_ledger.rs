use massa_execution_exports::ExecutionError;
use massa_hash::hash::Hash;
use massa_ledger::{Applicable, FinalLedger, LedgerChanges};
use massa_models::{Address, Amount};
use std::sync::{Arc, RwLock};

/// represents a speculative ledger state combining
/// data from the final ledger, previous speculative changes,
/// and accumulated changes since the construction of the object
pub struct SpeculativeLedger {
    /// final ledger
    final_ledger: Arc<RwLock<FinalLedger>>,

    /// accumulation of previous changes
    /// TODO maybe have the history directly here,
    /// so that we can avoid accumulating all the changes at every slot
    /// but only lazily query addresses backwards in history (to avoid useless computations) with caching
    previous_changes: LedgerChanges,

    /// list of added changes
    added_changes: LedgerChanges,
}

impl SpeculativeLedger {
    /// creates a new SpeculativeLedger
    pub fn new(final_ledger: Arc<RwLock<FinalLedger>>, previous_changes: LedgerChanges) -> Self {
        SpeculativeLedger {
            final_ledger,
            previous_changes,
            added_changes: Default::default(),
        }
    }

    /// takes the added changes (move) and resets added changes
    pub fn take(&mut self) -> LedgerChanges {
        std::mem::take(&mut self.added_changes)
    }

    /// takes a snapshot (clone) of the added changes
    pub fn get_snapshot(&self) -> LedgerChanges {
        self.added_changes.clone()
    }

    /// resets to a snapshot of added ledger changes
    pub fn reset_to_snapshot(&mut self, snapshot: LedgerChanges) {
        self.added_changes = snapshot;
    }

    /// gets the parallel balance of an address
    pub fn get_parallel_balance(&self, addr: &Address) -> Option<Amount> {
        // try to read from added_changes, then previous_changes, then final_ledger
        self.added_changes.get_parallel_balance_or_else(addr, || {
            self.previous_changes
                .get_parallel_balance_or_else(addr, || {
                    self.final_ledger
                        .read()
                        .expect("couldn't r-lock final ledger")
                        .get_parallel_balance(addr)
                })
        })
    }

    /// gets the bytecode of an address
    pub fn get_bytecode(&self, addr: &Address) -> Option<Vec<u8>> {
        // try to read from added_changes, then previous_changes, then final_ledger
        self.added_changes.get_bytecode_or_else(addr, || {
            self.previous_changes.get_bytecode_or_else(addr, || {
                self.final_ledger
                    .read()
                    .expect("couldn't r-lock final ledger")
                    .get_bytecode(addr)
            })
        })
    }

    /// Transfers parallel coins from one address to another.
    /// No changes are retained in case of failure.
    /// The spending address, if defined, must exist
    ///
    /// # parameters
    /// * from_addr: optional spending address (use None for pure coin creation)
    /// * to_addr: optional crediting address (use None for pure coin destruction)
    /// * amount: amount of coins to transfer
    pub fn transfer_parallel_coins(
        &mut self,
        from_addr: Option<Address>,
        to_addr: Option<Address>,
        amount: Amount,
    ) -> Result<(), ExecutionError> {
        // init empty ledger changes
        let mut changes = LedgerChanges::default();

        // spend coins from sender address (if any)
        if let Some(from_addr) = from_addr {
            let new_balance = self
                .get_parallel_balance(&from_addr)
                .ok_or(ExecutionError::RuntimeError(
                    "source address not found".into(),
                ))?
                .checked_sub(amount)
                .ok_or(ExecutionError::RuntimeError(
                    "unsufficient from_addr balance".into(),
                ))?;
            changes.set_parallel_balance(from_addr, new_balance);
        }

        // credit coins to destination address (if any)
        // note that to_addr can be the same as from_addr
        if let Some(to_addr) = to_addr {
            let new_balance = changes
                .get_parallel_balance_or_else(&to_addr, || self.get_parallel_balance(&to_addr))
                .unwrap_or(AMOUNT_ZERO)
                .checked_add(amount)
                .ok_or(ExecutionError::RuntimeError(
                    "overflow in to_addr balance".into(),
                ))?;
            changes.set_parallel_balance(to_addr, new_balance);
        }

        // apply changes
        self.added_changes.apply(changes);

        Ok(())
    }

    /// checks if an address exists
    pub fn entry_exists(&self, addr: &Address) -> bool {
        // try to read from added_changes, then previous_changes, then final_ledger
        self.added_changes.entry_exists_or_else(addr, || {
            self.previous_changes.entry_exists_or_else(addr, || {
                self.final_ledger
                    .read()
                    .expect("couldn't r-lock final ledger")
                    .entry_exists(addr)
            })
        })
    }

    /// creates a new smart contract address with initial bytecode
    pub fn create_new_sc_address(
        &mut self,
        addr: Address,
        bytecode: Vec<u8>,
    ) -> Result<(), ExecutionError> {
        // set bytecode (create if non-existant)
        Ok(self.added_changes.set_bytecode(addr, bytecode))
    }

    /// sets the bytecode of an address
    /// fails if the address doesn't exist
    #[allow(dead_code)] // TODO remove when it is used
    pub fn set_bytecode(&mut self, addr: Address, bytecode: Vec<u8>) -> Result<(), ExecutionError> {
        // check for existence
        if !self.entry_exists(&addr) {
            return Err(ExecutionError::RuntimeError(format!(
                "could not set bytecode for address {}: entry does not exist",
                addr
            )));
        }

        //set bytecode
        self.added_changes.set_bytecode(addr, bytecode);

        Ok(())
    }

    /// gets a copy of a data entry for a given address
    pub fn get_data_entry(&self, addr: &Address, key: &Hash) -> Option<Vec<u8>> {
        // try to read from added_changes, then previous_changes, then final_ledger
        self.added_changes.get_data_entry_or_else(addr, key, || {
            self.previous_changes.get_data_entry_or_else(addr, key, || {
                self.final_ledger
                    .read()
                    .expect("couldn't r-lock final ledger")
                    .get_data_entry(addr, key)
            })
        })
    }

    /// checks if a data entry exists for a given address
    pub fn has_data_entry(&self, addr: &Address, key: &Hash) -> bool {
        // try to read from added_changes, then previous_changes, then final_ledger
        self.added_changes.has_data_entry_or_else(addr, key, || {
            self.previous_changes.has_data_entry_or_else(addr, key, || {
                self.final_ledger
                    .read()
                    .expect("couldn't r-lock final ledger")
                    .has_data_entry(addr, key)
            })
        })
    }

    /// sets an entry for an address
    /// fails if the address doesn't exist
    pub fn set_data_entry(
        &mut self,
        addr: &Address,
        key: Hash,
        data: Vec<u8>,
    ) -> Result<(), ExecutionError> {
        // check for address existence
        if !self.entry_exists(&addr) {
            return Err(ExecutionError::RuntimeError(format!(
                "could not set data for address {}: entry does not exist",
                addr
            )));
        }

        // set data
        self.added_changes.set_data_entry(*addr, key, data);

        Ok(())
    }
}
