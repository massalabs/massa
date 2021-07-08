use crate::error::InternalError;
use sled::Tree;
use std::{
    collections::{HashMap, HashSet},
    convert::TryInto,
    usize,
};

use crate::{ConsensusConfig, ConsensusError};
use models::{
    array_from_slice, Address, DeserializeCompact, SerializationContext, SerializeCompact,
    SerializeVarInt,
};
use models::{DeserializeVarInt, Operation};
use serde::{Deserialize, Serialize};

struct Ledger {
    ledger_per_thread: Vec<Tree>, // containing (Address, LedgerData)
    latest_final_periods: Tree,   // containing (thread_number: u8, latest_final_period: u64)
    cfg: ConsensusConfig,
    context: SerializationContext,
}

#[derive(Debug)]
pub struct LedgerData {
    balance: u64,
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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LedgerChange {
    balance_delta: u64,
    balance_increment: bool, // wether to increment or decrement balance of delta
}

impl LedgerChange {
    fn chain(&mut self, change: &LedgerChange) -> Result<(), ConsensusError> {
        if self.balance_increment == change.balance_increment {
            self.balance_delta = self.balance_delta + change.balance_delta;
        } else {
            if change.balance_delta > self.balance_delta {
                self.balance_increment = !self.balance_increment;
            }
            self.balance_delta = change.balance_delta - self.balance_delta;
        }
        Ok(())
    }
}

impl LedgerData {
    fn apply_change(&mut self, change: &LedgerChange) -> Result<(), ConsensusError> {
        if change.balance_increment {
            self.balance = self.balance + change.balance_delta;
        } else {
            if change.balance_delta > self.balance {
                self.balance = self.balance - change.balance_delta;
            } else {
                return Err(ConsensusError::InvalidLedgerChange(
                    "negative balance".to_string(),
                ));
            }
        }
        Ok(())
    }
}

trait OerationLedgerInterface {
    fn get_involved_addresses(fee_target: &Address) -> HashSet<Address>;
    fn get_changes(
        fee_target: &Address,
    ) -> Result<Vec<HashMap<Address, LedgerChange>>, ConsensusError>;
}

impl OerationLedgerInterface for Operation {
    fn get_involved_addresses(fee_target: &Address) -> HashSet<Address> {
        todo!()
    }

    fn get_changes(
        fee_target: &Address,
    ) -> Result<Vec<HashMap<Address, LedgerChange>>, ConsensusError> {
        todo!()
    }
}

impl Ledger {
    /// if no latest_final_periods in file, they are initialized at 0u64
    fn new(cfg: ConsensusConfig, context: SerializationContext) -> Result<Ledger, ConsensusError> {
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
                latest_final_periods.insert([thread], &[0u8; 4])?; // 0u64
            }
        }
        Ok(Ledger {
            ledger_per_thread,
            latest_final_periods,
            cfg,
            context,
        })
    }

    /// Returns the final balance of an address. 0 if the address does not exist.
    fn get_final_balance(&self, address: &Address) -> Result<u64, ConsensusError> {
        let thread = address.get_thread(self.cfg.thread_count);
        if let Some(res) = self.ledger_per_thread[thread as usize].get(address.to_bytes())? {
            Ok(LedgerData::from_bytes_compact(&res, &self.context)?
                .0
                .balance)
        } else {
            Ok(0)
        }
    }

    /// Atomically apply a batch of changes to the ledger.
    /// All changes should occure in one thread.
    /// Update last final period.
    ///
    /// * If the balance of an address falls exactly to 0, it is removed from the ledger.
    /// * If the balance of a non-existing address increases, the address is added to the ledger.
    /// * If we attempt to substract more than the balance of an address, the transaction is cancelled and the function returns an error.
    fn apply_final_changes(
        &mut self,
        thread: u8,
        changes: Vec<(Address, LedgerChange)>,
        latest_final_period: u64,
    ) -> Result<(), ConsensusError> {
        let ledger = self.ledger_per_thread.get(thread as usize).ok_or(
            ConsensusError::LedgerInconsistency(format!("missing ledger for thread {:?}", thread)),
        )?;
        ledger.transaction(|db| {
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
            Ok(())
        })?;
        self.latest_final_periods.transaction(|db| {
            db.insert(&[thread], &latest_final_period.to_be_bytes())
                .map_err(|err| {
                    sled::transaction::ConflictableTransactionError::Abort(
                        InternalError::TransactionError(format!(
                            "error inserting transaction: {:?}",
                            err
                        )),
                    )
                })
        })?;
        Ok(())
    }

    /// returns the final periods if they are saved
    fn get_latest_final_periods(&self) -> Result<Option<Vec<u64>>, ConsensusError> {
        let mut res = Vec::new();
        for thread in 0..self.cfg.thread_count {
            if let Some(val) = self.latest_final_periods.get([thread])? {
                res.push(u64::from_be_bytes(array_from_slice(&val)?))
            } else {
                return Ok(None);
            }
        }
        Ok(Some(res))
    }

    /// To empty the db
    fn clear(&self) -> Result<(), ConsensusError> {
        for db in self.ledger_per_thread.iter() {
            db.clear()?;
        }
        self.latest_final_periods.clear()?;
        Ok(())
    }

    /// Used for bootstrap
    fn read_whole(&self) -> Result<Vec<HashMap<Address, LedgerData>>, ConsensusError> {
        let mut res = Vec::new();
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
