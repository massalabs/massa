use massa_models::Operation;
// Copyright (c) 2022 MASSA LABS <info@massa.net>
use massa_models::ledger_models::{LedgerChange, LedgerChanges, LedgerData};
use massa_models::prehash::{BuildMap, Map, Set};
use massa_models::{
    array_from_slice, constants::ADDRESS_SIZE_BYTES, Address, Amount, DeserializeCompact,
    DeserializeVarInt, SerializeCompact, SerializeVarInt,
};
use serde::{Deserialize, Serialize};
use sled::{Transactional, Tree};
use std::{
    collections::hash_map,
    convert::{TryFrom, TryInto},
    usize,
};

use crate::{
    error::{GraphError, InternalError, LedgerError, LedgerResult as Result},
    settings::LedgerConfig,
};

/// Here we map an address to its balance.
/// When a balance becomes final it is written on the disk.
pub struct Ledger {
    /// containing `(Address, LedgerData)`, one per thread
    ledger_per_thread: Vec<Tree>,
    /// containing `(thread_number: u8, latest_final_period: u64)`
    latest_final_periods: Tree,
    /// consensus related configuration
    cfg: LedgerConfig,
}

/// Read the initial ledger.
pub async fn read_genesis_ledger(ledger_config: &LedgerConfig) -> Result<Ledger> {
    // load ledger from file
    let ledger = serde_json::from_str::<LedgerSubset>(
        &tokio::fs::read_to_string(&ledger_config.initial_ledger_path).await?,
    )?;
    Ledger::new(ledger_config.to_owned(), Some(ledger))
}

/// Ledger specific method on operations
pub trait OperationLedgerInterface {
    /// Retrieve and aggregate ledger specific changes in the context of a block
    ///
    /// # Arguments
    /// * block creator
    /// * included endorsement producers
    /// * creator of the endorsed block
    /// * thread count (fixed by the configuration)
    /// * roll price (fixed by the configuration)
    /// * max expected number of endorsements (fixed by the configuration)
    fn get_ledger_changes(
        &self,
        creator: Address,
        endorsers: Vec<Address>,
        parent_creator: Address,
        roll_price: Amount,
        endorsement_count: u32,
    ) -> Result<LedgerChanges>;
}

impl OperationLedgerInterface for Operation {
    fn get_ledger_changes(
        &self,
        creator: Address,
        endorsers: Vec<Address>,
        parent_creator: Address,
        roll_price: Amount,
        endorsement_count: u32,
    ) -> Result<LedgerChanges> {
        let mut res = LedgerChanges::default();

        // sender fee
        let sender_address = Address::from_public_key(&self.sender_public_key);
        res.apply(
            &sender_address,
            &LedgerChange {
                balance_delta: self.fee,
                balance_increment: false,
            },
        )?;

        // fee target
        res.add_reward(
            creator,
            endorsers,
            parent_creator,
            self.fee,
            endorsement_count,
        )?;

        // operation type specific
        match &self.op {
            massa_models::OperationType::Transaction {
                recipient_address,
                amount,
            } => {
                res.apply(
                    &sender_address,
                    &LedgerChange {
                        balance_delta: (*amount),
                        balance_increment: false,
                    },
                )?;
                res.apply(
                    recipient_address,
                    &LedgerChange {
                        balance_delta: (*amount),
                        balance_increment: true,
                    },
                )?;
            }
            massa_models::OperationType::RollBuy { roll_count } => {
                res.apply(
                    &sender_address,
                    &LedgerChange {
                        balance_delta: roll_price
                            .checked_mul_u64(*roll_count)
                            .ok_or(LedgerError::AmountOverflowError)?,
                        balance_increment: false,
                    },
                )?;
            }
            // roll sale is handled separately with a delay
            massa_models::OperationType::RollSell { .. } => {}
            massa_models::OperationType::ExecuteSC {
                max_gas,
                gas_price,
                coins,
                ..
            } => {
                res.apply(
                    &sender_address,
                    &LedgerChange {
                        balance_delta: gas_price
                            .checked_mul_u64(*max_gas)
                            .ok_or(LedgerError::AmountOverflowError)?
                            .checked_add(*coins)
                            .ok_or(LedgerError::AmountOverflowError)?,
                        balance_increment: false,
                    },
                )?;
            }
            massa_models::OperationType::CallSC {
                max_gas,
                gas_price,
                sequential_coins,
                ..
            } => {
                res.apply(
                    &sender_address,
                    &LedgerChange {
                        balance_delta: gas_price
                            .checked_mul_u64(*max_gas)
                            .ok_or(LedgerError::AmountOverflowError)?
                            .checked_add(*sequential_coins)
                            .ok_or(LedgerError::AmountOverflowError)?,
                        balance_increment: false,
                    },
                )?;
            }
        }

        Ok(res)
    }
}

impl Ledger {
    /// if no `latest_final_periods` in file, they are initialized at `0u64`
    /// if there is a ledger in the given file, it is loaded
    pub fn new(cfg: LedgerConfig, opt_init_data: Option<LedgerSubset>) -> Result<Ledger> {
        let sled_config = sled::Config::default()
            .path(&cfg.ledger_path)
            .cache_capacity(cfg.ledger_cache_capacity)
            .flush_every_ms(cfg.ledger_flush_interval.map(|v| v.to_millis()));
        let db = sled_config.open()?;

        let mut ledger_per_thread = Vec::new();
        for thread in 0..cfg.thread_count {
            db.drop_tree(format!("ledger_thread_{}", thread))?;
            let current_tree = db.open_tree(format!("ledger_thread_{}", thread))?;
            ledger_per_thread.push(current_tree);
        }
        db.drop_tree("latest_final_periods")?;
        let latest_final_periods = db.open_tree("latest_final_periods")?;
        if latest_final_periods.is_empty() {
            for thread in 0..cfg.thread_count {
                let zero: u64 = 0;
                latest_final_periods.insert([thread], &zero.to_be_bytes())?;
            }
        }

        if let Some(init_ledger) = opt_init_data {
            ledger_per_thread.transaction(|ledger| {
                for (address, data) in init_ledger.0.iter() {
                    let thread = address.get_thread(cfg.thread_count);
                    ledger[thread as usize].insert(
                        &address.to_bytes(),
                        data.to_bytes_compact().map_err(|err| {
                            sled::transaction::ConflictableTransactionError::Abort(
                                InternalError::TransactionError(format!(
                                    "error serializing ledger data: {}",
                                    err
                                )),
                            )
                        })?,
                    )?;
                }
                Ok(())
            })?;
        }
        Ok(Ledger {
            ledger_per_thread,
            latest_final_periods,
            cfg,
        })
    }

    /// Returns the final ledger data of a list of unique addresses belonging to any thread.
    pub fn get_final_data(&self, addresses: Set<Address>) -> Result<LedgerSubset> {
        self.ledger_per_thread
            .transaction(|ledger_per_thread| {
                let mut result = LedgerSubset::default();
                for address in addresses.iter() {
                    let thread = address.get_thread(self.cfg.thread_count);
                    let ledger = ledger_per_thread.get(thread as usize).ok_or_else(|| {
                        sled::transaction::ConflictableTransactionError::Abort(
                            InternalError::TransactionError(format!(
                                "Could not get ledger for thread {}",
                                thread
                            )),
                        )
                    })?;
                    let data = if let Some(res) = ledger.get(address.to_bytes())? {
                        LedgerData::from_bytes_compact(&res)
                            .map_err(|err| {
                                sled::transaction::ConflictableTransactionError::Abort(
                                    InternalError::TransactionError(format!(
                                        "error deserializing ledger data: {}",
                                        err
                                    )),
                                )
                            })?
                            .0
                    } else {
                        LedgerData::default()
                    };

                    // Should never panic since we are operating on a set of addresses.
                    assert!(result.0.insert(*address, data).is_none());
                }
                Ok(result)
            })
            .map_err(|_| {
                LedgerError::LedgerInconsistency(format!(
                    "Unable to fetch data for addresses {:?}",
                    addresses
                ))
            })
    }

    /// If there is something in the ledger file, it is overwritten
    pub fn from_export(
        export: LedgerSubset,
        latest_final_periods: Vec<u64>,
        cfg: LedgerConfig,
    ) -> Result<Ledger> {
        let ledger = Ledger::new(cfg.clone(), None)?;
        ledger.clear()?;

        // fill ledger per thread
        for (address, addr_data) in export.0.iter() {
            let thread = address.get_thread(cfg.thread_count);
            if ledger.ledger_per_thread[thread as usize]
                .insert(address.into_bytes(), addr_data.to_bytes_compact()?)?
                .is_some()
            {
                return Err(LedgerError::LedgerInconsistency(format!(
                    "address {} already in ledger while bootstrapping",
                    address
                )));
            };
        }

        // initialize final periods
        ledger.latest_final_periods.transaction(|tree| {
            for (thread, period) in latest_final_periods.iter().enumerate() {
                tree.insert(&[thread as u8], &period.to_be_bytes())?;
            }
            Ok(())
        })?;
        Ok(ledger)
    }

    /// Returns the final balance of an address. 0 if the address does not exist.
    pub fn get_final_balance(&self, address: &Address) -> Result<Amount> {
        let thread = address.get_thread(self.cfg.thread_count);
        if let Some(res) = self.ledger_per_thread[thread as usize].get(address.to_bytes())? {
            Ok(LedgerData::from_bytes_compact(&res)?.0.balance)
        } else {
            Ok(Amount::default())
        }
    }

    /// Atomically apply a batch of changes to the ledger.
    /// All changes should occur in one thread.
    /// Update last final period.
    ///
    /// * If the balance of an address falls exactly to 0, it is removed from the ledger.
    /// * If the balance of a non-existing address increases, the address is added to the ledger.
    /// * If we attempt to subtract more than the balance of an address, the transaction is canceled and the function returns an error.
    pub fn apply_final_changes(
        &self,
        thread: u8,
        changes: &LedgerChanges,
        latest_final_period: u64,
    ) -> Result<()> {
        let ledger = self.ledger_per_thread.get(thread as usize).ok_or_else(|| {
            LedgerError::LedgerInconsistency(format!("missing ledger for thread {}", thread))
        })?;

        (ledger, &self.latest_final_periods).transaction(|(db, latest_final_periods_db)| {
            for (address, change) in changes.0.iter() {
                if address.get_thread(self.cfg.thread_count) != thread {
                    continue;
                }
                let address_bytes = address.to_bytes();
                let mut data = if let Some(old_bytes) = &db.get(address_bytes)? {
                    let (old, _) = LedgerData::from_bytes_compact(old_bytes).map_err(|err| {
                        sled::transaction::ConflictableTransactionError::Abort(
                            InternalError::TransactionError(format!(
                                "error deserializing ledger data: {}",
                                err
                            )),
                        )
                    })?;
                    old
                } else {
                    // creating new entry
                    LedgerData::default()
                };
                data.apply_change(change).map_err(|err| {
                    sled::transaction::ConflictableTransactionError::Abort(
                        InternalError::TransactionError(format!("error applying change: {}", err)),
                    )
                })?;
                // remove entry if nil
                if data.is_nil() {
                    db.remove(&address_bytes)?;
                } else {
                    db.insert(
                        &address_bytes,
                        data.to_bytes_compact().map_err(|err| {
                            sled::transaction::ConflictableTransactionError::Abort(
                                InternalError::TransactionError(format!(
                                    "error serializing ledger data: {}",
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
                            "error inserting transaction: {}",
                            err
                        )),
                    )
                })?;
            Ok(())
        })?;
        Ok(())
    }

    /// returns the final periods.
    pub fn get_latest_final_periods(&self) -> Result<Vec<u64>> {
        self.latest_final_periods
            .transaction(|db| {
                let mut res = Vec::with_capacity(self.cfg.thread_count as usize);
                for thread in 0..self.cfg.thread_count {
                    if let Some(val) = db.get([thread])? {
                        let latest = array_from_slice(&val).map_err(|err| {
                            sled::transaction::ConflictableTransactionError::Abort(
                                InternalError::TransactionError(format!(
                                    "error getting latest final period for thread: {} {}",
                                    thread, err
                                )),
                            )
                        })?;
                        res.push(u64::from_be_bytes(latest));
                    } else {
                        // Note: this should never happen,
                        // since they are initialized in ::new().
                        return Err(sled::transaction::ConflictableTransactionError::Abort(
                            InternalError::TransactionError(format!(
                                "error getting latest final period for thread: {}",
                                thread
                            )),
                        ));
                    }
                }
                Ok(res)
            })
            .map_err(|_| {
                LedgerError::LedgerInconsistency("Unable to fetch latest final periods.".into())
            })
    }

    /// To empty the db.
    pub fn clear(&self) -> Result<()> {
        // Note: this cannot be done transactionally.
        for db in self.ledger_per_thread.iter() {
            db.clear()?;
        }
        self.latest_final_periods.clear()?;
        Ok(())
    }

    /// Used for bootstrap.
    /// Note: this cannot be done transactionally.
    pub fn read_whole(&self) -> Result<LedgerSubset> {
        let mut res = LedgerSubset::default();
        for tree in self.ledger_per_thread.iter() {
            for element in tree.iter() {
                let (addr, data) = element?;
                let address = Address::from_bytes(addr.as_ref().try_into()?)?;
                let (ledger_data, _) = LedgerData::from_bytes_compact(&data)?;
                if let Some(val) = res.0.insert(address, ledger_data) {
                    return Err(LedgerError::LedgerInconsistency(format!(
                        "address {:?} twice in ledger",
                        val
                    )));
                }
            }
        }
        Ok(res)
    }

    /// Gets ledger at latest final blocks for `query_addrs`
    pub fn get_final_ledger_subset(&self, query_addrs: &Set<Address>) -> Result<LedgerSubset> {
        let res = self.ledger_per_thread.transaction(|ledger_per_thread| {
            let mut data = LedgerSubset::default();
            for addr in query_addrs {
                let thread = addr.get_thread(self.cfg.thread_count);
                if let Some(data_bytes) = ledger_per_thread[thread as usize].get(addr.to_bytes())? {
                    let (ledger_data, _) =
                        LedgerData::from_bytes_compact(&data_bytes).map_err(|err| {
                            sled::transaction::ConflictableTransactionError::Abort(
                                InternalError::TransactionError(format!(
                                    "error deserializing ledger data: {}",
                                    err
                                )),
                            )
                        })?;
                    data.0.insert(*addr, ledger_data);
                } else {
                    data.0.insert(*addr, LedgerData::default());
                }
            }
            Ok(data)
        })?;
        Ok(res)
    }
}

/// address to ledger data map
/// Only part of a ledger
#[derive(Clone, Debug, Serialize, Deserialize, Default)]
pub struct LedgerSubset(pub Map<Address, LedgerData>);

impl LedgerSubset {
    /// If subset contains given address
    pub fn contains(&self, address: &Address) -> bool {
        self.0.contains_key(address)
    }

    /// Get the data for given address
    pub fn get_data(&self, address: &Address) -> LedgerData {
        self.0.get(address).cloned().unwrap_or(LedgerData {
            balance: Amount::default(),
        })
    }

    /// List involved addresses
    pub fn get_involved_addresses(&self) -> Set<Address> {
        self.0.keys().copied().collect()
    }

    /// Applies given change to that ledger subset
    /// note: a failure may still leave the entry modified
    pub fn apply_change(&mut self, addr: &Address, change: &LedgerChange) -> Result<()> {
        match self.0.entry(*addr) {
            hash_map::Entry::Occupied(mut occ) => {
                occ.get_mut().apply_change(change)?;
                if occ.get().is_nil() {
                    occ.remove();
                }
            }
            hash_map::Entry::Vacant(vac) => {
                let mut res = LedgerData::default();
                res.apply_change(change)?;
                if !res.is_nil() {
                    vac.insert(res);
                }
            }
        }
        Ok(())
    }

    /// apply ledger changes
    ///  note: a failure may still leave the entry modified
    pub fn apply_changes(&mut self, changes: &LedgerChanges) -> Result<()> {
        for (addr, change) in changes.0.iter() {
            self.apply_change(addr, change)?;
        }
        Ok(())
    }

    /// Applies thread changes change to that ledger subset
    /// note: a failure may still leave the entry modified
    pub fn chain(&mut self, changes: &LedgerChanges) -> Result<()> {
        for (addr, change) in changes.0.iter() {
            self.apply_change(addr, change)?;
        }
        Ok(())
    }

    /// merge another ledger subset into self, overwriting existing data
    /// address that are in not other are removed from self
    pub fn sync_from(&mut self, addrs: &Set<Address>, mut other: LedgerSubset) {
        for addr in addrs.iter() {
            if let Some(new_val) = other.0.remove(addr) {
                self.0.insert(*addr, new_val);
            } else {
                self.0.remove(addr);
            }
        }
    }

    /// clone subset
    #[must_use]
    pub fn clone_subset(&self, addrs: &Set<Address>) -> Self {
        LedgerSubset(
            self.0
                .iter()
                .filter_map(|(a, dta)| {
                    if addrs.contains(a) {
                        Some((*a, *dta))
                    } else {
                        None
                    }
                })
                .collect(),
        )
    }
}

impl<'a> TryFrom<&'a Ledger> for LedgerSubset {
    type Error = GraphError;

    fn try_from(value: &'a Ledger) -> Result<Self, Self::Error> {
        Ok(LedgerSubset(
            value
                .read_whole()?
                .0
                .iter()
                .map(|(k, v)| (*k, *v))
                .collect(),
        ))
    }
}

impl SerializeCompact for LedgerSubset {
    /// ## Example
    /// ```rust
    /// # use massa_models::{SerializeCompact, DeserializeCompact, SerializationContext, Address, Amount};
    /// # use std::str::FromStr;
    /// # use massa_models::ledger_models::LedgerData;
    /// # use massa_graph::ledger::LedgerSubset;
    /// # let ledger = LedgerSubset(vec![
    /// #   (Address::from_bs58_check("2oxLZc6g6EHfc5VtywyPttEeGDxWq3xjvTNziayWGDfxETZVTi".into()).unwrap(), LedgerData::new(Amount::from_str("1022").unwrap())),
    /// #   (Address::from_bs58_check("2mvD6zEvo8gGaZbcs6AYTyWKFonZaKvKzDGRsiXhZ9zbxPD11q".into()).unwrap(), LedgerData::new(Amount::from_str("1020").unwrap())),
    /// # ].into_iter().collect());
    /// # massa_models::init_serialization_context(massa_models::SerializationContext::default());
    /// let bytes = ledger.clone().to_bytes_compact().unwrap();
    /// let (res, _) = LedgerSubset::from_bytes_compact(&bytes).unwrap();
    /// for (address, data) in &ledger.0 {
    ///    assert!(res.0.iter().filter(|(addr, dta)| &address == addr && dta.to_bytes_compact().unwrap() == data.to_bytes_compact().unwrap()).count() == 1)
    /// }
    /// assert_eq!(ledger.0.len(), res.0.len());
    /// ```
    fn to_bytes_compact(&self) -> Result<Vec<u8>, massa_models::ModelsError> {
        let mut res: Vec<u8> = Vec::new();

        let entry_count: u64 = self.0.len().try_into().map_err(|err| {
            massa_models::ModelsError::SerializeError(format!(
                "too many entries in LedgerSubset: {}",
                err
            ))
        })?;
        res.extend(entry_count.to_varint_bytes());
        for (address, data) in self.0.iter() {
            res.extend(&address.to_bytes());
            res.extend(&data.to_bytes_compact()?);
        }

        Ok(res)
    }
}

impl DeserializeCompact for LedgerSubset {
    fn from_bytes_compact(buffer: &[u8]) -> Result<(Self, usize), massa_models::ModelsError> {
        let mut cursor = 0usize;

        let (entry_count, delta) = u64::from_varint_bytes(&buffer[cursor..])?;
        // TODO: add entry_count checks ... see #1200
        cursor += delta;

        let mut ledger_subset = LedgerSubset(Map::with_capacity_and_hasher(
            entry_count as usize,
            BuildMap::default(),
        ));
        for _ in 0..entry_count {
            let address = Address::from_bytes(&array_from_slice(&buffer[cursor..])?)?;
            cursor += ADDRESS_SIZE_BYTES;

            let (data, delta) = LedgerData::from_bytes_compact(&buffer[cursor..])?;
            cursor += delta;

            ledger_subset.0.insert(address, data);
        }

        Ok((ledger_subset, cursor))
    }
}

#[cfg(test)]
mod tests {
    use std::str::FromStr;

    use serial_test::serial;

    use super::*;

    #[test]
    #[serial]
    fn test_ledger_change_chain() {
        for &v1 in &[-100i32, -10, 0, 10, 100] {
            for &v2 in &[-100i32, -10, 0, 10, 100] {
                let mut res = LedgerChange {
                    balance_increment: (v1 >= 0),
                    balance_delta: Amount::from_str(&v1.abs().to_string()).unwrap(),
                };
                res.chain(&LedgerChange {
                    balance_increment: (v2 >= 0),
                    balance_delta: Amount::from_str(&v2.abs().to_string()).unwrap(),
                })
                .unwrap();
                let expect: i32 = v1 + v2;
                assert_eq!(res.balance_increment, (expect >= 0));
                assert_eq!(
                    res.balance_delta,
                    Amount::from_str(&expect.abs().to_string()).unwrap()
                );
            }
        }
    }
}
