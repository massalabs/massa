use crate::error::InternalError;
use sled::{Transactional, Tree};
use std::{
    collections::{hash_map, HashMap, HashSet},
    convert::TryInto,
    usize,
};

use crate::{ConsensusConfig, ConsensusError};
use models::{
    array_from_slice, u8_from_slice, Address, DeserializeCompact, SerializationContext,
    SerializeCompact, SerializeVarInt,
};
use models::{DeserializeVarInt, Operation};
use serde::{Deserialize, Serialize};

pub struct Ledger {
    ledger_per_thread: Vec<Tree>, // containing (Address, LedgerData)
    latest_final_periods: Tree,   // containing (thread_number: u8, latest_final_period: u64)
    cfg: ConsensusConfig,
    context: SerializationContext,
}

#[derive(Debug)]
pub struct LedgerData {
    balance: u64,
}

impl LedgerData {
    pub fn get_balance(&self) -> u64 {
        self.balance
    }
}

impl SerializeCompact for LedgerData {
    fn to_bytes_compact(
        &self,
        context: &models::SerializationContext,
    ) -> Result<Vec<u8>, models::ModelsError> {
        let mut res: Vec<u8> = Vec::new();
        res.extend(self.balance.to_varint_bytes());
        Ok(res)
    }
}

impl DeserializeCompact for LedgerData {
    fn from_bytes_compact(
        buffer: &[u8],
        context: &models::SerializationContext,
    ) -> Result<(Self, usize), models::ModelsError> {
        let mut cursor = 0usize;
        let (balance, delta) = u64::from_varint_bytes(&buffer[cursor..])?;
        cursor += delta;
        Ok((LedgerData { balance }, cursor))
    }
}

impl LedgerData {
    fn apply_change(&mut self, change: &LedgerChange) -> Result<(), ConsensusError> {
        if change.balance_increment {
            self.balance = self.balance.checked_add(change.balance_delta).ok_or(
                ConsensusError::InvalidLedgerChange(
                    "balance overflow in LedgerData::apply_change".into(),
                ),
            )?;
        } else {
            self.balance = self.balance.checked_sub(change.balance_delta).ok_or(
                ConsensusError::InvalidLedgerChange(
                    "balance underflow in LedgerData::apply_change".into(),
                ),
            )?;
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LedgerChange {
    pub balance_delta: u64,
    pub balance_increment: bool, // wether to increment or decrement balance of delta
}

impl LedgerChange {
    pub fn new(balance_delta: u64, balance_increment: bool) -> Self {
        LedgerChange {
            balance_delta,
            balance_increment,
        }
    }

    fn chain(&mut self, change: &LedgerChange) -> Result<(), ConsensusError> {
        if self.balance_increment == change.balance_increment {
            self.balance_delta = self.balance_delta.checked_add(change.balance_delta).ok_or(
                ConsensusError::InvalidLedgerChange("overflow in LedgerChange::chain".into()),
            )?;
        } else if change.balance_delta > self.balance_delta {
            self.balance_delta = change.balance_delta.checked_sub(self.balance_delta).ok_or(
                ConsensusError::InvalidLedgerChange("underflow in LedgerChange::chain".into()),
            )?;
            self.balance_increment = !self.balance_increment;
        } else {
            self.balance_delta = self.balance_delta.checked_sub(change.balance_delta).ok_or(
                ConsensusError::InvalidLedgerChange("underflow in LedgerChange::chain".into()),
            )?;
        }
        if self.balance_delta == 0 {
            self.balance_increment = true;
        }
        Ok(())
    }
}

impl SerializeCompact for LedgerChange {
    fn to_bytes_compact(
        &self,
        context: &models::SerializationContext,
    ) -> Result<Vec<u8>, models::ModelsError> {
        let mut res: Vec<u8> = Vec::new();
        res.extend(self.balance_delta.to_varint_bytes());
        if self.balance_increment {
            res.push(1);
        } else {
            res.push(0);
        }
        Ok(res)
    }
}

impl DeserializeCompact for LedgerChange {
    fn from_bytes_compact(
        buffer: &[u8],
        context: &models::SerializationContext,
    ) -> Result<(Self, usize), models::ModelsError> {
        let mut cursor = 0usize;
        let (balance_delta, delta) = u64::from_varint_bytes(&buffer[cursor..])?;
        cursor += delta;

        let balance_increment_u8 = u8_from_slice(&buffer)?;
        cursor += 1;
        let balance_increment = if balance_increment_u8 == 0 {
            false
        } else {
            true
        };

        Ok((
            LedgerChange {
                balance_delta,
                balance_increment,
            },
            cursor,
        ))
    }
}

trait OperationLedgerInterface {
    fn get_involved_addresses(
        &self,
        fee_target: &Address,
    ) -> Result<HashSet<Address>, ConsensusError>;
    fn get_changes(
        &self,
        fee_target: &Address,
        thread_count: u8,
    ) -> Result<Vec<HashMap<Address, LedgerChange>>, ConsensusError>;
}

impl OperationLedgerInterface for Operation {
    fn get_involved_addresses(
        &self,
        fee_target: &Address,
    ) -> Result<HashSet<Address>, ConsensusError> {
        let mut res = HashSet::new();
        res.insert(fee_target.clone());
        res.insert(Address::from_public_key(&self.content.sender_public_key)?);
        match self.content.op {
            models::OperationType::Transaction {
                recipient_address, ..
            } => {
                res.insert(recipient_address);
            }
        }
        Ok(res)
    }

    fn get_changes(
        &self,
        fee_target: &Address,
        thread_count: u8,
    ) -> Result<Vec<HashMap<Address, LedgerChange>>, ConsensusError> {
        let mut changes: Vec<HashMap<Address, LedgerChange>> =
            vec![HashMap::new(); thread_count as usize];

        let mut try_add_changes =
            |address: &Address, change: LedgerChange| -> Result<(), ConsensusError> {
                let thread = address.get_thread(thread_count);
                match changes[thread as usize].entry(*address) {
                    hash_map::Entry::Occupied(mut occ) => {
                        occ.get_mut().chain(&change)?;
                    }
                    hash_map::Entry::Vacant(vac) => {
                        vac.insert(change);
                    }
                }
                Ok(())
            };

        // sender fee
        let sender_address = Address::from_public_key(&self.content.sender_public_key)?;
        try_add_changes(
            &sender_address,
            LedgerChange {
                balance_delta: self.content.fee,
                balance_increment: false,
            },
        )?;

        // fee target
        try_add_changes(
            fee_target,
            LedgerChange {
                balance_delta: self.content.fee,
                balance_increment: true,
            },
        )?;

        // operation type specific
        match self.content.op {
            models::OperationType::Transaction {
                recipient_address,
                amount,
            } => {
                try_add_changes(
                    &sender_address,
                    LedgerChange {
                        balance_delta: amount,
                        balance_increment: false,
                    },
                )?;
                try_add_changes(
                    &recipient_address,
                    LedgerChange {
                        balance_delta: amount,
                        balance_increment: true,
                    },
                )?;
            }
        }

        Ok(changes)
    }
}

impl Ledger {
    /// if no latest_final_periods in file, they are initialized at 0u64
    pub fn new(
        cfg: ConsensusConfig,
        context: SerializationContext,
    ) -> Result<Ledger, ConsensusError> {
        let sled_config = sled::Config::default()
            .path(&cfg.ledger_path)
            .cache_capacity(cfg.ledger_cache_capacity)
            .flush_every_ms(cfg.ledger_flush_interval.map(|v| v.to_millis()));
        let db = sled_config.open()?;
        let mut ledger_per_thread = Vec::new();
        for thread in 0..cfg.thread_count {
            let current_tree = db.open_tree(format!("ledger_thread_{:?}", thread))?;
            ledger_per_thread.push(current_tree);
        }
        let latest_final_periods = db.open_tree("latest_final_periods")?;
        if latest_final_periods.is_empty() {
            for thread in 0..cfg.thread_count {
                let zero: u64 = 0;
                latest_final_periods.insert([thread], &zero.to_be_bytes())?;
            }
        }
        Ok(Ledger {
            ledger_per_thread,
            latest_final_periods,
            cfg,
            context,
        })
    }

    /// Returns the final ledger data of a list of unique addresses belonging to any thread.
    pub fn get_final_datas(
        &self,
        mut addresses: HashSet<&Address>,
    ) -> Result<HashMap<Address, LedgerData>, ConsensusError> {
        // TODO: only run the transaction on a subset of relevant ledgers?
        self.ledger_per_thread
            .transaction(|ledger_per_thread| {
                let mut result = HashMap::with_capacity(addresses.len());
                for address in addresses.iter() {
                    let thread = address.get_thread(self.cfg.thread_count);
                    let ledger = ledger_per_thread.get(thread as usize).ok_or(
                        sled::transaction::ConflictableTransactionError::Abort(
                            InternalError::TransactionError(format!(
                                "Could not get ledger for thread {:?}",
                                thread
                            )),
                        ),
                    )?;
                    let data = if let Some(res) = ledger.get(address.to_bytes())? {
                        LedgerData::from_bytes_compact(&res, &self.context)
                            .map_err(|err| {
                                sled::transaction::ConflictableTransactionError::Abort(
                                    InternalError::TransactionError(format!(
                                        "error deserializing ledger data: {:?}",
                                        err
                                    )),
                                )
                            })?
                            .0
                    } else {
                        LedgerData { balance: 0 }
                    };

                    // Should never panic since we are operating on a set of addresses.
                    assert!(result.insert((*address).clone(), data).is_none());
                }
                Ok(result)
            })
            .map_err(|_| {
                ConsensusError::LedgerInconsistency(format!(
                    "Unable to fetch data for addresses {:?}",
                    addresses
                ))
            })
    }

    /// Check that the right thread for the address was specified.
    fn check_thread_for_address(
        &self,
        thread: u8,
        address: &Address,
    ) -> Result<(), ConsensusError> {
        let address_thread = address.get_thread(self.cfg.thread_count);
        if address_thread != thread {
            return Err(ConsensusError::LedgerInconsistency(format!(
                "Wrong thread for address {:?}",
                address
            )));
        }
        Ok(())
    }

    /// Atomically apply a batch of changes to the ledger.
    /// All changes should occure in one thread.
    /// Update last final period.
    ///
    /// * If the balance of an address falls exactly to 0, it is removed from the ledger.
    /// * If the balance of a non-existing address increases, the address is added to the ledger.
    /// * If we attempt to substract more than the balance of an address, the transaction is cancelled and the function returns an error.
    pub fn apply_final_changes(
        &self,
        thread: u8,
        changes: Vec<(Address, LedgerChange)>,
        latest_final_period: u64,
    ) -> Result<(), ConsensusError> {
        // First, check that the thread is correct for all changes.
        for (address, _) in changes.iter() {
            self.check_thread_for_address(thread, address)?;
        }

        let ledger = self.ledger_per_thread.get(thread as usize).ok_or(
            ConsensusError::LedgerInconsistency(format!("missing ledger for thread {:?}", thread)),
        )?;

        (ledger, &self.latest_final_periods).transaction(|(db, latest_final_periods_db)| {
            for (address, change) in changes.iter() {
                let address_bytes = address.to_bytes();
                let mut data = if let Some(old_bytes) = &db.get(address_bytes)? {
                    let (old, _) = LedgerData::from_bytes_compact(old_bytes, &self.context)
                        .map_err(|err| {
                            sled::transaction::ConflictableTransactionError::Abort(
                                InternalError::TransactionError(format!(
                                    "error deserializing ledger data: {:?}",
                                    err
                                )),
                            )
                        })?;
                    old
                } else {
                    // creating new entry
                    LedgerData { balance: 0 }
                };
                data.apply_change(&change).map_err(|err| {
                    sled::transaction::ConflictableTransactionError::Abort(
                        InternalError::TransactionError(format!(
                            "error applying change: {:?}",
                            err
                        )),
                    )
                })?;
                // remove entry if balance is at 0
                if data.balance == 0 {
                    db.remove(&address_bytes)?;
                } else {
                    db.insert(
                        &address_bytes,
                        data.to_bytes_compact(&self.context).map_err(|err| {
                            sled::transaction::ConflictableTransactionError::Abort(
                                InternalError::TransactionError(format!(
                                    "error serializing ledger data: {:?}",
                                    err
                                )),
                            )
                        })?,
                    )?;
                }
            }
            latest_final_periods_db
                .insert(&[thread], &latest_final_period.to_be_bytes())
                .map_err(|err| {
                    sled::transaction::ConflictableTransactionError::Abort(
                        InternalError::TransactionError(format!(
                            "error inserting transaction: {:?}",
                            err
                        )),
                    )
                })?;
            Ok(())
        })?;
        Ok(())
    }

    /// returns the final periods.
    pub fn get_latest_final_periods(&self) -> Result<Vec<u64>, ConsensusError> {
        self.latest_final_periods
            .transaction(|db| {
                let mut res = Vec::with_capacity(self.cfg.thread_count as usize);
                for thread in 0..self.cfg.thread_count {
                    if let Some(val) = db.get([thread])? {
                        let latest = array_from_slice(&val).map_err(|err| {
                            sled::transaction::ConflictableTransactionError::Abort(
                                InternalError::TransactionError(format!(
                                    "error getting latest final period for thread: {:?}",
                                    thread
                                )),
                            )
                        })?;
                        res.push(u64::from_be_bytes(latest));
                    } else {
                        // Note: this should never happen,
                        // since they are initialized in ::new().
                        return Err(sled::transaction::ConflictableTransactionError::Abort(
                            InternalError::TransactionError(format!(
                                "error getting latest final period for thread: {:?}",
                                thread
                            )),
                        ));
                    }
                }
                Ok(res)
            })
            .map_err(|_| {
                ConsensusError::LedgerInconsistency("Unable to fetch latest final periods.".into())
            })
    }

    /// To empty the db.
    pub fn clear(&self) -> Result<(), ConsensusError> {
        // Note: this cannot be done transactionally.
        for db in self.ledger_per_thread.iter() {
            db.clear()?;
        }
        self.latest_final_periods.clear()?;
        Ok(())
    }

    /// Used for bootstrap.
    pub fn read_whole(&self) -> Result<Vec<HashMap<Address, LedgerData>>, ConsensusError> {
        // Note: this cannot be done transactionally.
        let mut res = Vec::with_capacity(self.cfg.thread_count as usize);
        for tree in self.ledger_per_thread.iter() {
            let mut map = HashMap::new();
            for element in tree.iter() {
                let (addr, data) = element?;
                let address = Address::from_bytes(addr.as_ref().try_into()?)?;
                let (ledger_data, _) = LedgerData::from_bytes_compact(&data, &self.context)?;
                if let Some(val) = map.insert(address, ledger_data) {
                    return Err(ConsensusError::LedgerInconsistency(format!(
                        "address {:?} twice in ledger",
                        val
                    )));
                }
            }
            res.push(map);
        }
        Ok(res)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ledger_change_chain() {
        for &v1 in &[-100i32, -10, 0, 10, 100] {
            for &v2 in &[-100i32, -10, 0, 10, 100] {
                let mut res = LedgerChange {
                    balance_increment: (v1 >= 0),
                    balance_delta: v1.abs() as u64,
                };
                res.chain(&LedgerChange {
                    balance_increment: (v2 >= 0),
                    balance_delta: v2.abs() as u64,
                })
                .unwrap();
                let expect: i32 = v1 + v2;
                assert_eq!(res.balance_increment, (expect >= 0));
                assert_eq!(res.balance_delta, expect.abs() as u64);
            }
        }
    }
}
