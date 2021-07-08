use crate::error::InternalError;
use sled::{Transactional, Tree};
use std::{
    collections::{hash_map, HashMap, HashSet},
    convert::{TryFrom, TryInto},
    usize,
};

use crate::{ConsensusConfig, ConsensusError};
use models::{
    array_from_slice, u8_from_slice, Address, DeserializeCompact, ModelsError, SerializeCompact,
    SerializeVarInt, ADDRESS_SIZE_BYTES,
};
use models::{DeserializeVarInt, Operation};
use serde::{Deserialize, Serialize};

pub struct Ledger {
    ledger_per_thread: Vec<Tree>, // containing (Address, LedgerData)
    latest_final_periods: Tree,   // containing (thread_number: u8, latest_final_period: u64)
    cfg: ConsensusConfig,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct LedgerData {
    pub balance: u64,
}

impl SerializeCompact for LedgerData {
    fn to_bytes_compact(&self) -> Result<Vec<u8>, models::ModelsError> {
        let mut res: Vec<u8> = Vec::new();
        res.extend(self.balance.to_varint_bytes());
        Ok(res)
    }
}

impl DeserializeCompact for LedgerData {
    fn from_bytes_compact(buffer: &[u8]) -> Result<(Self, usize), models::ModelsError> {
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
    /// New ledger change with given delta and increment
    pub fn new(balance_delta: u64, balance_increment: bool) -> Self {
        LedgerChange {
            balance_delta,
            balance_increment,
        }
    }

    /// Combines the effects of two ledger changes
    /// Takes the first one mutably
    pub fn chain(&mut self, change: &LedgerChange) -> Result<(), ConsensusError> {
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
    fn to_bytes_compact(&self) -> Result<Vec<u8>, models::ModelsError> {
        let mut res: Vec<u8> = Vec::new();
        res.push(if self.balance_increment { 1u8 } else { 0u8 });
        res.extend(self.balance_delta.to_varint_bytes());
        Ok(res)
    }
}

impl DeserializeCompact for LedgerChange {
    fn from_bytes_compact(buffer: &[u8]) -> Result<(Self, usize), models::ModelsError> {
        let mut cursor = 0usize;

        let balance_increment = match u8_from_slice(&buffer[cursor..])? {
            0u8 => false,
            1u8 => true,
            _ => {
                return Err(ModelsError::DeserializeError(
                    "wrong boolean balance_increment encoding in LedgerChange deserialization"
                        .into(),
                ))
            }
        };
        cursor += 1;

        let (balance_delta, delta) = u64::from_varint_bytes(&buffer[cursor..])?;
        cursor += delta;

        Ok((
            LedgerChange {
                balance_increment,
                balance_delta,
            },
            cursor,
        ))
    }
}

pub trait OperationLedgerInterface {
    fn get_ledger_changes(
        &self,
        fee_target: &Address,
        thread_count: u8,
        roll_price: u64,
    ) -> Result<Vec<HashMap<Address, LedgerChange>>, ConsensusError>;
}

impl OperationLedgerInterface for Operation {
    fn get_ledger_changes(
        &self,
        fee_target: &Address,
        thread_count: u8,
        roll_price: u64,
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
            models::OperationType::RollBuy { roll_count } => {
                try_add_changes(
                    &sender_address,
                    LedgerChange {
                        balance_delta: roll_count
                            .checked_mul(roll_price)
                            .ok_or(ConsensusError::RollOverflowError)?,
                        balance_increment: false,
                    },
                )?;
            }
            // roll sale is handled separately with a delay
            models::OperationType::RollSell { .. } => {}
        }

        Ok(changes)
    }
}

impl Ledger {
    /// if no latest_final_periods in file, they are initialized at 0u64
    /// if there is a ledger in the given file, it is loaded
    pub fn new(
        cfg: ConsensusConfig,
        datas: Option<HashMap<Address, LedgerData>>,
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

        if let Some(map) = datas {
            ledger_per_thread.transaction(|ledger| {
                for (address, data) in map.iter() {
                    let thread = address.get_thread(cfg.thread_count);
                    ledger[thread as usize].insert(
                        &address.to_bytes(),
                        data.to_bytes_compact().map_err(|err| {
                            sled::transaction::ConflictableTransactionError::Abort(
                                InternalError::TransactionError(format!(
                                    "error serializing ledger data: {:?}",
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
    pub fn get_final_data(
        &self,
        addresses: HashSet<&Address>,
    ) -> Result<HashMap<Address, LedgerData>, ConsensusError> {
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
                        LedgerData::from_bytes_compact(&res)
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

    /// If there is something in the ledger file, it is overwritten
    pub fn from_export(
        export: LedgerExport,
        latest_final_periods: Vec<u64>,
        cfg: ConsensusConfig,
    ) -> Result<Ledger, ConsensusError> {
        let ledger = Ledger::new(cfg.clone(), None)?;
        ledger.clear()?;

        // fill ledger per thread
        for thread in 0..cfg.thread_count {
            for (address, balance) in export.ledger_per_thread[thread as usize].iter() {
                if let Some(_) = ledger.ledger_per_thread[thread as usize]
                    .insert(address.into_bytes(), balance.to_bytes_compact()?)?
                {
                    return Err(ConsensusError::LedgerInconsistency(format!(
                        "adress {:?} already in ledger while bootsrapping",
                        address
                    )));
                };
            }
        }
        // initilize final periods
        ledger.latest_final_periods.transaction(|tree| {
            for (thread, period) in latest_final_periods.iter().enumerate() {
                tree.insert(&[thread as u8], &period.to_be_bytes())?;
            }
            Ok(())
        })?;
        Ok(ledger)
    }

    /// Returns the final balance of an address. 0 if the address does not exist.
    pub fn get_final_balance(&self, address: &Address) -> Result<u64, ConsensusError> {
        let thread = address.get_thread(self.cfg.thread_count);
        if let Some(res) = self.ledger_per_thread[thread as usize].get(address.to_bytes())? {
            Ok(LedgerData::from_bytes_compact(&res)?.0.balance)
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
                    let (old, _) = LedgerData::from_bytes_compact(old_bytes).map_err(|err| {
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
                        data.to_bytes_compact().map_err(|err| {
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
                                    "error getting latest final period for thread: {:?} {:?}",
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
    // Note: this cannot be done transactionally.
    pub fn read_whole(&self) -> Result<Vec<Vec<(Address, LedgerData)>>, ConsensusError> {
        let mut res = Vec::with_capacity(self.cfg.thread_count as usize);
        for tree in self.ledger_per_thread.iter() {
            let mut map = HashMap::new();
            for element in tree.iter() {
                let (addr, data) = element?;
                let address = Address::from_bytes(addr.as_ref().try_into()?)?;
                let (ledger_data, _) = LedgerData::from_bytes_compact(&data)?;
                if let Some(val) = map.insert(address, ledger_data) {
                    return Err(ConsensusError::LedgerInconsistency(format!(
                        "address {:?} twice in ledger",
                        val
                    )));
                }
            }
            res.push(map.into_iter().collect());
        }
        Ok(res)
    }

    /// Gets ledger at latest final blocks for query_addrs
    pub fn get_final_ledger_subset(
        &self,
        query_addrs: &HashSet<Address>,
    ) -> Result<LedgerSubset, ConsensusError> {
        let data = self.ledger_per_thread.transaction(|ledger_per_thread| {
            let mut data = vec![HashMap::new(); self.cfg.thread_count as usize];
            for addr in query_addrs {
                let thread = addr.get_thread(self.cfg.thread_count);
                if let Some(data_bytes) = ledger_per_thread[thread as usize].get(addr.to_bytes())? {
                    let (ledger_data, _) =
                        LedgerData::from_bytes_compact(&data_bytes).map_err(|err| {
                            sled::transaction::ConflictableTransactionError::Abort(
                                InternalError::TransactionError(format!(
                                    "error deserializing ledger data: {:?}",
                                    err
                                )),
                            )
                        })?;
                    data[thread as usize].insert(addr.clone().clone(), ledger_data);
                } else {
                    data[thread as usize].insert(addr.clone().clone(), LedgerData { balance: 0 });
                }
            }
            Ok(data)
        })?;
        Ok(LedgerSubset { data })
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct LedgerSubset {
    pub data: Vec<HashMap<Address, LedgerData>>,
}

impl LedgerSubset {
    /// Create an empty ledger subset
    pub fn new(thread_count: u8) -> Self {
        LedgerSubset {
            data: vec![HashMap::new(); thread_count as usize],
        }
    }

    /// If subset contains given address
    pub fn contains(&self, address: &Address, thread_count: u8) -> bool {
        self.data[address.get_thread(thread_count) as usize].contains_key(address)
    }

    /// Get the data for given address
    pub fn get_data(&self, address: &Address, thread_count: u8) -> &LedgerData {
        let thread = address.get_thread(thread_count);
        self.data[thread as usize]
            .get(address)
            .unwrap_or(&LedgerData { balance: 0 })
    }

    /// Applies given change to that ledger subset
    pub fn apply_change(
        &mut self,
        (change_address, change): (&Address, &LedgerChange),
    ) -> Result<(), ConsensusError> {
        let thread_count = self.data.len() as u8;
        self.data[change_address.get_thread(thread_count) as usize]
            .entry(*change_address)
            .or_insert_with(|| LedgerData { balance: 0 })
            .apply_change(change)
    }

    /// apply batch of changes only if all changes
    /// in that batch passed, leave self untouched on failure
    pub fn try_apply_changes(
        &mut self,
        changes: &Vec<HashMap<Address, LedgerChange>>,
    ) -> Result<(), ConsensusError> {
        // copy involved addresses
        let mut new_ledger = LedgerSubset {
            data: self
                .data
                .iter()
                .enumerate()
                .map(|(thread, thread_data)| {
                    thread_data
                        .iter()
                        .filter_map(|(addr, addr_data)| {
                            if changes[thread].contains_key(addr) {
                                return Some((*addr, addr_data.clone()));
                            }
                            None
                        })
                        .collect()
                })
                .collect(),
        };
        // try applying changes to the new ledger
        for thread_changes in changes.iter() {
            for thread_change in thread_changes.iter() {
                new_ledger.apply_change(thread_change)?;
            }
        }
        // overwrite updated addresses in self
        self.extend(new_ledger);
        Ok(())
    }

    /// merge another ledger subset into self
    pub fn extend(&mut self, other: LedgerSubset) {
        for (self_thread_data, other_thread_data) in
            self.data.iter_mut().zip(other.data.into_iter())
        {
            self_thread_data.extend(other_thread_data)
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LedgerExport {
    pub ledger_per_thread: Vec<Vec<(Address, LedgerData)>>, // containing (Address, LedgerData)
}

impl<'a> TryFrom<&'a Ledger> for LedgerExport {
    type Error = ConsensusError;

    fn try_from(value: &'a Ledger) -> Result<Self, Self::Error> {
        Ok(LedgerExport {
            ledger_per_thread: value.read_whole()?.iter().cloned().collect(),
        })
    }
}

impl LedgerExport {
    /// Empty ledger export used for tests
    pub fn new(thread_count: u8) -> Self {
        LedgerExport {
            ledger_per_thread: vec![Vec::new(); thread_count as usize],
        }
    }
}

impl SerializeCompact for LedgerExport {
    /// ## Example
    /// ```rust
    /// # use models::{SerializeCompact, DeserializeCompact, SerializationContext, Address};
    /// # use consensus::{LedgerExport, LedgerData};
    /// # let mut ledger = LedgerExport::new(2);
    /// # ledger.ledger_per_thread = vec![
    /// #   vec![(Address::from_bs58_check("2oxLZc6g6EHfc5VtywyPttEeGDxWq3xjvTNziayWGDfxETZVTi".into()).unwrap(), LedgerData{balance: 1022})],
    /// #   vec![(Address::from_bs58_check("2mvD6zEvo8gGaZbcs6AYTyWKFonZaKvKzDGRsiXhZ9zbxPD11q".into()).unwrap(), LedgerData{balance: 1020})],
    /// # ];
    /// # models::init_serialization_context(models::SerializationContext {
    /// #     max_block_operations: 1024,
    /// #     parent_count: 2,
    /// #     max_peer_list_length: 128,
    /// #     max_message_size: 3 * 1024 * 1024,
    /// #     max_block_size: 3 * 1024 * 1024,
    /// #     max_bootstrap_blocks: 100,
    /// #     max_bootstrap_cliques: 100,
    /// #     max_bootstrap_deps: 100,
    /// #     max_bootstrap_children: 100,
    /// #     max_ask_blocks_per_message: 10,
    /// #     max_operations_per_message: 1024,
    /// #     max_bootstrap_message_size: 100000000,
    /// # });
    /// let bytes = ledger.clone().to_bytes_compact().unwrap();
    /// let (res, _) = LedgerExport::from_bytes_compact(&bytes).unwrap();
    /// for thread in 0..2 {
    ///     for (address, data) in &ledger.ledger_per_thread[thread] {
    ///        assert!(res.ledger_per_thread[thread].iter().filter(|(addr, dta)| address == addr && dta.to_bytes_compact().unwrap() == data.to_bytes_compact().unwrap()).count() == 1)
    ///     }
    ///     assert_eq!(ledger.ledger_per_thread[thread].len(), res.ledger_per_thread[thread].len());
    /// }
    /// ```
    fn to_bytes_compact(&self) -> Result<Vec<u8>, models::ModelsError> {
        let mut res: Vec<u8> = Vec::new();

        let thread_count: u32 = self.ledger_per_thread.len().try_into().map_err(|err| {
            models::ModelsError::SerializeError(format!(
                "too many threads in LedgerExport: {:?}",
                err
            ))
        })?;
        res.extend(u32::from(thread_count).to_varint_bytes());
        for thread_ledger in self.ledger_per_thread.iter() {
            let vec_count: u32 = thread_ledger.len().try_into().map_err(|err| {
                models::ModelsError::SerializeError(format!(
                    "too many threads in LedgerExport: {:?}",
                    err
                ))
            })?;

            res.extend(u32::from(vec_count).to_varint_bytes());
            for (address, data) in thread_ledger.iter() {
                res.extend(&address.to_bytes());
                res.extend(&data.to_bytes_compact()?);
            }
        }

        Ok(res)
    }
}

impl DeserializeCompact for LedgerExport {
    fn from_bytes_compact(buffer: &[u8]) -> Result<(Self, usize), models::ModelsError> {
        let mut cursor = 0usize;

        let (thread_count, delta) = u32::from_varint_bytes(&buffer[cursor..])?;
        cursor += delta;

        let mut ledger_per_thread = Vec::with_capacity(thread_count as usize);
        for _ in 0..(thread_count as usize) {
            let (vec_count, delta) = u32::from_varint_bytes(&buffer[cursor..])?;
            cursor += delta;
            let mut set: Vec<(Address, LedgerData)> = Vec::with_capacity(vec_count as usize);

            for _ in 0..(vec_count as usize) {
                let address = Address::from_bytes(&array_from_slice(&buffer[cursor..])?)?;
                cursor += ADDRESS_SIZE_BYTES;

                let (data, delta) = LedgerData::from_bytes_compact(&buffer[cursor..])?;
                cursor += delta;
                set.push((address, data));
            }

            ledger_per_thread.push(set);
        }

        Ok((LedgerExport { ledger_per_thread }, cursor))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serial_test::serial;

    #[test]
    #[serial]
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
