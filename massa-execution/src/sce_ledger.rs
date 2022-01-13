use crate::types::Bytecode;
use crate::ExecutionError;
use massa_hash::hash::Hash;
use massa_hash::HASH_SIZE_BYTES;
use massa_models::hhasher::BuildHHasher;
use massa_models::{address::AddressHashMap, hhasher::HHashMap, Address, Amount, AMOUNT_ZERO};
use massa_models::{
    array_from_slice, DeserializeCompact, DeserializeVarInt, ModelsError, SerializeCompact,
    SerializeVarInt, Slot, ADDRESS_SIZE_BYTES,
};
use serde::{Deserialize, Serialize};

/// an entry in the SCE ledger
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SCELedgerEntry {
    // SCE balance
    pub balance: Amount,

    // optional executable module
    pub opt_module: Option<Bytecode>,

    // datastore
    pub data: HHashMap<Hash, Vec<u8>>,
}

impl SCELedgerEntry {
    /// applies an entry update to self
    pub fn apply_entry_update(&mut self, update: &SCELedgerEntryUpdate) {
        // balance
        if let Some(new_balance) = update.update_balance {
            self.balance = new_balance;
        }

        // module
        self.opt_module = update.update_opt_module.clone();

        // data
        for (data_key, data_update) in update.update_data.iter() {
            match data_update {
                Some(new_data) => {
                    self.data.insert(*data_key, new_data.clone());
                }
                None => {
                    self.data.remove(data_key);
                }
            }
        }
    }
}

impl SerializeCompact for SCELedgerEntry {
    fn to_bytes_compact(&self) -> Result<Vec<u8>, massa_models::ModelsError> {
        let mut res: Vec<u8> = Vec::new();

        // write balance
        res.extend(self.balance.to_bytes_compact()?);

        // write opt module data
        if let Some(module_data) = &self.opt_module {
            // write that it is present
            res.push(1);

            // write length
            let length: u32 = module_data.len().try_into().map_err(|_| {
                ModelsError::SerializeError(
                    "SCE ledger entry module data too long for serialization".into(),
                )
            })?;
            // TODO check against max length
            res.extend(length.to_varint_bytes());

            // write bytecode
            res.extend(module_data);
        } else {
            // write that it is absent
            res.push(0);
        }

        // write data store

        // write length
        let length: u32 = self.data.len().try_into().map_err(|_| {
            ModelsError::SerializeError(
                "SCE ledger entry data store too long for serialization".into(),
            )
        })?;
        // TODO limit length
        res.extend(length.to_varint_bytes());

        // write entry pairs
        for (h, data_entry) in self.data.iter() {
            // write hash
            res.extend(h.to_bytes());

            // write length
            let length: u32 = data_entry.len().try_into().map_err(|_| {
                ModelsError::SerializeError(
                    "SCE ledger entry data store entry too long for serialization".into(),
                )
            })?;
            // TODO check against max length
            res.extend(length.to_varint_bytes());

            // write data entry
            res.extend(data_entry);
        }

        Ok(res)
    }
}

impl DeserializeCompact for SCELedgerEntry {
    fn from_bytes_compact(buffer: &[u8]) -> Result<(Self, usize), massa_models::ModelsError> {
        let mut cursor = 0usize;

        // read balance
        let (balance, delta) = Amount::from_bytes_compact(&buffer[cursor..])?;
        cursor += delta;

        // read opt module data
        let has_module = match buffer.get(cursor) {
            Some(1) => true,
            Some(0) => false,
            _ => {
                return Err(ModelsError::DeserializeError(
                    "could not deserialize ledger entry opt module data byte".into(),
                ))
            }
        };
        cursor += 1;
        let opt_module: Option<Bytecode> = if has_module {
            // read length
            let (length, delta) = u32::from_varint_bytes(&buffer[cursor..])?;
            // TOOD limit length with from_varint_bytes_bounded
            cursor += delta;

            // read items
            if let Some(slice) = buffer.get(cursor..(cursor + (length as usize))) {
                cursor += length as usize;
                Some(slice.to_vec())
            } else {
                return Err(ModelsError::DeserializeError(
                    "could not deserialize ledger entry module bytes: buffer too small".into(),
                ));
            }
        } else {
            None
        };

        // read data store

        // read length
        let (length, delta) = u32::from_varint_bytes(&buffer[cursor..])?;
        // TOOD limit length with from_varint_bytes_bounded
        cursor += delta;

        // read entry pairs
        let mut data: HHashMap<Hash, Vec<u8>> =
            HHashMap::with_capacity_and_hasher(length as usize, BuildHHasher::default());
        for _ in 0..length {
            // read hash
            let h = Hash::from_bytes(&array_from_slice(&buffer[cursor..])?)?;
            cursor += HASH_SIZE_BYTES;

            // read data length
            let (d_length, delta) = u32::from_varint_bytes(&buffer[cursor..])?;
            // TOOD limit d_length with from_varint_bytes_bounded
            cursor += delta;

            // read data
            let entry_data = if let Some(slice) = buffer.get(cursor..(cursor + (d_length as usize)))
            {
                cursor += d_length as usize;
                slice.to_vec()
            } else {
                return Err(ModelsError::DeserializeError(
                    "could not deserialize ledger entry data store entry bytes: buffer too small"
                        .into(),
                ));
            };

            // insert
            data.insert(h, entry_data);
        }

        Ok((
            SCELedgerEntry {
                balance,
                opt_module,
                data,
            },
            cursor,
        ))
    }
}

// optional updates to be applied to a ledger entry
#[derive(Debug, Clone, Default)]
pub struct SCELedgerEntryUpdate {
    pub update_balance: Option<Amount>,
    pub update_opt_module: Option<Bytecode>,
    pub update_data: HHashMap<Hash, Option<Vec<u8>>>, // None for row deletion
}

impl SCELedgerEntryUpdate {
    /// apply another SCELedgerEntryUpdate to self
    pub fn apply_entry_update(&mut self, other: &SCELedgerEntryUpdate) {
        // balance
        if let Some(new_balance) = other.update_balance {
            self.update_balance = Some(new_balance);
        }

        // module
        if let Some(new_opt_module) = &other.update_opt_module {
            self.update_opt_module = Some(new_opt_module.clone());
        }

        // data
        self.update_data.extend(other.update_data.clone());
    }
}

#[derive(Debug, Clone)]
pub enum SCELedgerChange {
    // delete an entry
    Delete,
    // sets an entry to an absolute value
    Set(SCELedgerEntry),
    // updates an entry
    Update(SCELedgerEntryUpdate),
}

impl Default for SCELedgerChange {
    fn default() -> Self {
        Self::Delete
    }
}

impl SCELedgerChange {
    /// applies another SCELedgerChange to the current one
    pub fn apply_change(&mut self, other: &SCELedgerChange) {
        let new_val = match (&self, other) {
            // other deletes the entry
            (_, SCELedgerChange::Delete) => {
                // make self delete as well
                SCELedgerChange::Delete
            }

            // other sets an absolute entry
            (_, new_set @ SCELedgerChange::Set(_)) => {
                // make self set the same absolute entry
                new_set.clone()
            }

            // self deletes, other updates
            (SCELedgerChange::Delete, SCELedgerChange::Update(other_entry_update)) => {
                // prepare a default entry
                let mut res_entry = SCELedgerEntry::default();
                // apply other's updates to res_entry
                res_entry.apply_entry_update(other_entry_update);
                // make self set to res_entry
                SCELedgerChange::Set(res_entry)
            }

            // self sets, other updates
            (SCELedgerChange::Set(cur_entry), SCELedgerChange::Update(other_entry_update)) => {
                // apply other's updates to cur_entry
                // TODO avoid clone, act directly on mutable cur_entry
                let mut res_entry = cur_entry.clone();
                res_entry.apply_entry_update(other_entry_update);
                SCELedgerChange::Set(res_entry)
            }

            // self updates, other updates
            (
                SCELedgerChange::Update(cur_entry_update),
                SCELedgerChange::Update(other_entry_update),
            ) => {
                // try to apply other's updates to self's updates
                // TODO avoid clone, act directly on mutable cur_entry_update
                let mut res_update = cur_entry_update.clone();
                res_update.apply_entry_update(other_entry_update);
                SCELedgerChange::Update(res_update)
            }
        };
        *self = new_val;
    }
}

/// SCE ledger
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SCELedger(pub AddressHashMap<SCELedgerEntry>);

impl SerializeCompact for SCELedger {
    fn to_bytes_compact(&self) -> Result<Vec<u8>, massa_models::ModelsError> {
        let mut res: Vec<u8> = Vec::new();

        // write length
        let length: u32 = self.0.len().try_into().map_err(|_| {
            ModelsError::SerializeError("SCE ledger too long for serialization".into())
        })?;
        // TODO limit length
        res.extend(length.to_varint_bytes());

        // write entry pairs
        for (addr, ledger_entry) in self.0.iter() {
            // write address
            res.extend(addr.to_bytes());

            // write ledger entry
            res.extend(ledger_entry.to_bytes_compact()?);
        }

        Ok(res)
    }
}

impl DeserializeCompact for SCELedger {
    fn from_bytes_compact(buffer: &[u8]) -> Result<(Self, usize), massa_models::ModelsError> {
        let mut cursor = 0usize;

        // read length
        let (length, delta) = u32::from_varint_bytes(&buffer[cursor..])?;
        // TOOD limit length with from_varint_bytes_bounded
        cursor += delta;

        // read entry pairs
        let mut res_ledger: AddressHashMap<SCELedgerEntry> =
            AddressHashMap::with_capacity_and_hasher(length as usize, BuildHHasher::default());
        for _ in 0..length {
            // read address
            let address = Address::from_bytes(&array_from_slice(&buffer[cursor..])?)?;
            cursor += ADDRESS_SIZE_BYTES;

            // read ledger entry
            let (ledger_entry, delta) = SCELedgerEntry::from_bytes_compact(&buffer[cursor..])?;
            cursor += delta;

            // add to output ledger
            res_ledger.insert(address, ledger_entry);
        }

        Ok((SCELedger(res_ledger), cursor))
    }
}

/// list of ledger changes (deletions, resets, updates)
#[derive(Debug, Clone, Default)]
pub struct SCELedgerChanges(pub AddressHashMap<SCELedgerChange>);

impl SCELedgerChanges {
    /// extends the current SCELedgerChanges with another
    pub fn apply_changes(&mut self, changes: &SCELedgerChanges) {
        for (addr, change) in changes.0.iter() {
            self.apply_change(*addr, change);
        }
    }

    /// appliees a single change to self
    pub fn apply_change(&mut self, addr: Address, change: &SCELedgerChange) {
        self.0
            .entry(addr)
            .and_modify(|cur_c| cur_c.apply_change(change))
            .or_insert_with(|| change.clone());
    }

    pub fn clear(&mut self) {
        self.0.clear();
    }
}

impl SCELedger {
    /// creates an SCELedger from a hashmap of balances
    pub fn from_balances_map(balances_map: AddressHashMap<Amount>) -> Self {
        SCELedger(
            balances_map
                .into_iter()
                .map(|(k, v)| {
                    (
                        k,
                        SCELedgerEntry {
                            balance: v,
                            ..Default::default()
                        },
                    )
                })
                .collect(),
        )
    }

    /// applies ledger changes to ledger
    pub fn apply_changes(&mut self, changes: &SCELedgerChanges) {
        for (addr, change) in changes.0.iter() {
            match change {
                // delete entry
                SCELedgerChange::Delete => {
                    self.0.remove(addr);
                }

                // set entry to absolute value
                SCELedgerChange::Set(new_entry) => {
                    self.0.insert(*addr, new_entry.clone());
                }

                // update entry
                SCELedgerChange::Update(update) => {
                    // insert default if absent
                    self.0
                        .entry(*addr)
                        .or_insert_with(SCELedgerEntry::default)
                        .apply_entry_update(update);
                }
            }
        }
    }
}

/// The final ledger.
#[derive(Debug, Clone)]
pub struct FinalLedger {
    /// The slot of the ledger.
    pub slot: Slot,
    /// The ledger.
    pub ledger: SCELedger,
}

/// represents an execution step from the point of view of the SCE ledger
/// applying cumulative_history_changes then caused_changes to final_ledger yields the current ledger during the ledger step
#[derive(Debug, Clone)]
pub struct SCELedgerStep {
    // The final ledger and its slot
    pub final_ledger_slot: FinalLedger,

    // accumulator of existing ledger changes
    pub cumulative_history_changes: SCELedgerChanges,

    // additional changes caused by the step
    pub caused_changes: SCELedgerChanges,
}

impl SCELedgerStep {
    /// gets the balance of an SCE ledger entry
    pub fn get_balance(&self, addr: &Address) -> Amount {
        // check if caused_changes or cumulative_history_changes have an update on this
        for changes in [&self.caused_changes, &self.cumulative_history_changes] {
            match changes.0.get(addr) {
                Some(SCELedgerChange::Delete) => return AMOUNT_ZERO,
                Some(SCELedgerChange::Set(new_entry)) => return new_entry.balance,
                Some(SCELedgerChange::Update(update)) => {
                    if let Some(updated_balance) = update.update_balance {
                        return updated_balance;
                    }
                }
                None => {}
            }
        }
        // check if the final ledger has the info
        if let Some(entry) = self.final_ledger_slot.ledger.0.get(addr) {
            return entry.balance;
        }
        // otherwise, just return zero
        AMOUNT_ZERO
    }

    /// sets the balance of an address
    pub fn set_balance(&mut self, addr: Address, balance: Amount) {
        let update = SCELedgerEntryUpdate {
            update_balance: Some(balance),
            update_opt_module: Default::default(),
            update_data: Default::default(),
        };
        self.caused_changes
            .apply_change(addr, &SCELedgerChange::Update(update));
    }

    /// tries to increase/decrease the balance of an address
    /// does not change anything on failure
    pub fn set_balance_delta(
        &mut self,
        addr: Address,
        amount: Amount,
        positive: bool,
    ) -> Result<(), ExecutionError> {
        let mut balance = self.get_balance(&addr);
        if positive {
            balance = balance
                .checked_add(amount)
                .ok_or_else(|| ModelsError::CheckedOperationError("balance overflow".into()))?;
        } else {
            balance = balance
                .checked_sub(amount)
                .ok_or_else(|| ModelsError::CheckedOperationError("balance underflow".into()))?;
        }
        self.set_balance(addr, balance);
        Ok(())
    }

    /// gets the module of an SCE ledger entry
    ///  returns None if the entry was not found or has no module
    pub fn get_module(&self, addr: &Address) -> Option<Bytecode> {
        // check if caused_changes or cumulative_history_changes have an update on this
        for changes in [&self.caused_changes, &self.cumulative_history_changes] {
            match changes.0.get(addr) {
                Some(SCELedgerChange::Delete) => return None,
                Some(SCELedgerChange::Set(new_entry)) => return new_entry.opt_module.clone(),
                Some(SCELedgerChange::Update(update)) => return update.update_opt_module.clone(),
                None => {}
            }
        }
        // check if the final ledger has the info
        match self.final_ledger_slot.ledger.0.get(addr) {
            Some(entry) => entry.opt_module.clone(),
            _ => None,
        }
    }

    /// returns a data entry
    ///   None if address not found or entry nto found in addr's data
    pub fn get_data_entry(&self, addr: &Address, key: &Hash) -> Option<Vec<u8>> {
        // check if caused_changes or cumulative_history_changes have an update on this
        for changes in [&self.caused_changes, &self.cumulative_history_changes] {
            match changes.0.get(addr) {
                Some(SCELedgerChange::Delete) => return None,
                Some(SCELedgerChange::Set(new_entry)) => return new_entry.data.get(key).cloned(),
                Some(SCELedgerChange::Update(update)) => {
                    match update.update_data.get(key) {
                        None => {}                 // no updates
                        Some(None) => return None, // data entry deleted,
                        Some(Some(updated_data)) => return Some(updated_data.clone()),
                    }
                }
                None => {}
            }
        }

        // check if the final ledger has the info
        match self.final_ledger_slot.ledger.0.get(addr) {
            Some(entry) => entry.data.get(key).cloned(),
            _ => None,
        }
    }

    /// checks if a data entry exists
    pub fn has_data_entry(&self, addr: &Address, key: &Hash) -> bool {
        // check if caused_changes or cumulative_history_changes have an update on this
        for changes in [&self.caused_changes, &self.cumulative_history_changes] {
            match changes.0.get(addr) {
                Some(SCELedgerChange::Delete) => return false,
                Some(SCELedgerChange::Set(_)) => return true,
                Some(SCELedgerChange::Update(update)) => {
                    match update.update_data.get(key) {
                        None => {}                  // no updates
                        Some(None) => return false, // data entry deleted,
                        Some(Some(_)) => return true,
                    }
                }
                None => {}
            }
        }

        // check if the final ledger has the info
        self.final_ledger_slot.ledger.0.contains_key(addr)
    }

    /// sets data entry
    pub fn set_data_entry(&mut self, addr: Address, key: Hash, value: Vec<u8>) {
        let update = SCELedgerEntryUpdate {
            update_data: [(key, Some(value))].into_iter().collect(),
            ..Default::default()
        };
        self.caused_changes
            .apply_change(addr, &SCELedgerChange::Update(update));
    }

    pub fn set_module(&mut self, addr: Address, module: Vec<u8>) {
        let update = SCELedgerEntryUpdate {
            update_opt_module: Some(module),
            ..Default::default()
        };
        self.caused_changes
            .apply_change(addr, &SCELedgerChange::Update(update));
    }
}
