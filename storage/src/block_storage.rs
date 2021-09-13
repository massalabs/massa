// Copyright (c) 2021 MASSA LABS <info@massa.net>

use crate::error::{InternalError, StorageError};
use models::{
    array_from_slice, Address, Block, BlockId, DeserializeCompact, OperationId,
    OperationSearchResult, OperationSearchResultBlockStatus, OperationSearchResultStatus,
    SerializeCompact, Slot, BLOCK_ID_SIZE_BYTES, OPERATION_ID_SIZE_BYTES,
};
use sled::{self, transaction::TransactionalTree, IVec, Transactional};
use std::{
    collections::{hash_map, HashMap, HashSet},
    convert::TryInto,
    sync::atomic::{AtomicUsize, Ordering},
    sync::Arc,
    usize,
};
use tokio::sync::{mpsc, Notify};

pub struct StorageCleaner {
    max_stored_blocks: usize,
    notify: Arc<Notify>,
    shutdown: mpsc::Receiver<()>,
    block_count: Arc<AtomicUsize>,
    hash_to_block: sled::Tree,
    slot_to_hash: sled::Tree,
    op_to_block: sled::Tree,   // op_id -> (block_id, index)
    addr_to_op: sled::Tree,    // address -> Vec<operationId>
    addr_to_block: sled::Tree, // addr -> Vec<BlockId>
}

impl StorageCleaner {
    pub fn new(
        max_stored_blocks: usize,
        notify: Arc<Notify>,
        shutdown: mpsc::Receiver<()>,
        hash_to_block: sled::Tree,
        slot_to_hash: sled::Tree,
        op_to_block: sled::Tree,
        addr_to_op: sled::Tree, // address -> Vec<operationId>
        addr_to_block: sled::Tree,
        block_count: Arc<AtomicUsize>,
    ) -> Result<Self, StorageError> {
        Ok(StorageCleaner {
            max_stored_blocks,
            notify,
            shutdown,
            block_count,
            hash_to_block,
            slot_to_hash,
            op_to_block,
            addr_to_op,
            addr_to_block,
        })
    }

    pub async fn run_loop(mut self) -> Result<(), StorageError> {
        loop {
            massa_trace!("storage.storage_cleaner.run_loop.select", {});

            // 1. run the cleaner.
            let mut current_count = self.block_count.load(Ordering::Acquire);
            let mut removed = 0;
            while current_count > self.max_stored_blocks {
                if let Some((slot, hash)) = self.slot_to_hash.first()? {
                    (
                        &self.slot_to_hash,
                        &self.hash_to_block,
                        &self.op_to_block,
                        &self.addr_to_op,
                        &self.addr_to_block
                    )
                        .transaction(|(slots, hashes, ops, addr_to_op, addr_to_block)| {
                            slots.remove(&slot)?;
                            let s_block = hashes
                                .remove(&hash)?
                                .ok_or_else( ||
                                    sled::transaction::ConflictableTransactionError::Abort(
                                        InternalError::TransactionError("block missing".into()),
                                    )
                                )?;
                            let (block, _) = Block::from_bytes_compact(
                                s_block.as_ref()
                            )
                            .map_err(|err| {
                                sled::transaction::ConflictableTransactionError::Abort(
                                    InternalError::TransactionError(format!(
                                        "error deserializing block: {:?}",
                                        err
                                    )),
                                )
                            })?;
                            let fee_addr = Address::from_public_key(&block.header.content.creator).map_err(|err| {
                                sled::transaction::ConflictableTransactionError::Abort(
                                    InternalError::TransactionError(format!(
                                        "error computing fee address: {:?}",
                                        err
                                    )),
                                )
                            })?;
                            let mut addr_to_op_cache: HashMap<Address, HashSet<OperationId>> = HashMap::new();
                            for op in block.operations.into_iter() {
                                // remove operation from op_id => block index
                                let op_id = op.get_operation_id().map_err(
                                    |err| {
                                        sled::transaction::ConflictableTransactionError::Abort(
                                            InternalError::TransactionError(format!(
                                                "error getting operation ID: {:?}",
                                                err
                                            )),
                                        )
                                    },
                                )?;
                                ops.remove(&op_id.to_bytes())?;

                                // remove involved addrs from address => operation index
                                let involved_addrs = op.get_ledger_involved_addresses(Some(fee_addr)).map_err(|err| {
                                    sled::transaction::ConflictableTransactionError::Abort(
                                        InternalError::TransactionError(format!(
                                            "error computing op-involved addresses: {:?}",
                                            err
                                        )),
                                    )
                                })?;
                                for involved_addr in involved_addrs.into_iter() {
                                    match addr_to_op_cache.entry(involved_addr) {
                                        hash_map::Entry::Occupied(mut occ) => { occ.get_mut().remove(&op_id); },
                                        hash_map::Entry::Vacant(vac) => {
                                            if let Some(ivec) = addr_to_op.get(&involved_addr.to_bytes())? {
                                                let mut inv_ops = ops_from_ivec(ivec)
                                                    .map_err(
                                                        |err| {
                                                            sled::transaction::ConflictableTransactionError::Abort(
                                                                InternalError::TransactionError(format!(
                                                                    "error loading operation IDs: {:?}",
                                                                    err
                                                                )),
                                                            )
                                                        },
                                                    )?;
                                                inv_ops.remove(&op_id);
                                                vac.insert(inv_ops);
                                            }
                                        },
                                    }
                                }
                            }
                            // write back addr_to_op_cache
                            for (addr, op_ids) in addr_to_op_cache.into_iter() {
                                if op_ids.is_empty() {
                                    // element is empty: remove it
                                    addr_to_op.remove(&addr.to_bytes())?;
                                    continue;
                                }
                                let ivec = ops_to_ivec(&op_ids)
                                    .map_err(
                                        |err| {
                                            sled::transaction::ConflictableTransactionError::Abort(
                                                InternalError::TransactionError(format!(
                                                    "error serializing operation IDs: {:?}",
                                                    err
                                                )),
                                            )
                                        },
                                    )?;
                                addr_to_op.insert(&addr.to_bytes(), ivec)?;
                            }

                            // update addr_to_block
                            let creator_addr = Address::from_public_key(&block.header.content.creator).map_err(|err| {
                                sled::transaction::ConflictableTransactionError::Abort(
                                    InternalError::TransactionError(format!(
                                        "error computing creator address: {:?}",
                                        err
                                    )),
                                )
                            })?;
                            if let Some(ivec_blocks) = addr_to_block.get(&creator_addr.to_bytes())? {
                                let mut ids = block_ids_from_ivec(ivec_blocks).map_err(|err| {
                                    sled::transaction::ConflictableTransactionError::Abort(
                                        InternalError::TransactionError(format!(
                                            "error deserializing block ids: {:?}",
                                            err
                                        )),
                                    )
                                })?;
                                ids.remove(&block.header.compute_block_id().map_err(|err| {
                                    sled::transaction::ConflictableTransactionError::Abort(
                                        InternalError::TransactionError(format!(
                                            "error computing block id: {:?}",
                                            err
                                        )),
                                    )
                                })?);
                                if ids.is_empty() {
                                    addr_to_block.remove(&creator_addr.to_bytes())?;
                                } else {
                                    let ivec = block_id_to_ivec(&ids).map_err(|err| {
                                        sled::transaction::ConflictableTransactionError::Abort(
                                            InternalError::TransactionError(format!(
                                                "error serializing block ids: {:?}",
                                                err
                                            )),
                                        )
                                    })?;
                                    addr_to_block.insert(&creator_addr.to_bytes(), ivec)?;
                                }
                            }
                            Ok(())
                        })?;
                }
                current_count -= 1;
                removed += 1;
            }
            if removed > 0 {
                self.block_count.fetch_sub(removed, Ordering::Release);
            }

            // 2. wait for wakeup
            tokio::select! {
                _ = self.notify.notified() => {
                    massa_trace!("storage.storage_cleaner.run_loop.notified", {});
                },
                _ = self.shutdown.recv() => {
                    massa_trace!("storage.storage_cleaner.run_loop.shutdown", {});
                    break;
                }
            }
        }

        Ok(())
    }
}

fn ops_from_ivec(buffer: IVec) -> Result<HashSet<OperationId>, StorageError> {
    let operation_count = buffer.len() / OPERATION_ID_SIZE_BYTES;
    let mut operations: HashSet<OperationId> = HashSet::with_capacity(operation_count as usize);
    for op_i in 0..operation_count {
        let cursor = OPERATION_ID_SIZE_BYTES * op_i;
        let operation_id = OperationId::from_bytes(&array_from_slice(&buffer[cursor..])?)?;
        operations.insert(operation_id);
    }
    Ok(operations)
}

fn ops_to_ivec(op_ids: &HashSet<OperationId>) -> Result<IVec, StorageError> {
    let mut res: Vec<u8> = Vec::with_capacity(OPERATION_ID_SIZE_BYTES * op_ids.len());
    for op_id in op_ids.iter() {
        res.extend(op_id.to_bytes());
    }
    Ok(res[..].into())
}

fn block_ids_from_ivec(buffer: IVec) -> Result<HashSet<BlockId>, StorageError> {
    let block_count = buffer.len() / BLOCK_ID_SIZE_BYTES;
    let mut blocks: HashSet<BlockId> = HashSet::with_capacity(block_count as usize);
    for id_i in 0..block_count {
        let cursor = BLOCK_ID_SIZE_BYTES * id_i;
        let block_id = BlockId::from_bytes(&array_from_slice(&buffer[cursor..])?)?;
        blocks.insert(block_id);
    }
    Ok(blocks)
}

fn block_id_to_ivec(block_ids: &HashSet<BlockId>) -> Result<IVec, StorageError> {
    let mut res: Vec<u8> = Vec::with_capacity(BLOCK_ID_SIZE_BYTES * block_ids.len());
    for block_id in block_ids.iter() {
        res.extend(block_id.to_bytes());
    }
    Ok(res[..].into())
}
#[derive(Clone)]
pub struct BlockStorage {
    block_count: Arc<AtomicUsize>,
    /// BlockId -> Block
    hash_to_block: sled::Tree,
    /// Slot -> BlockId
    slot_to_hash: sled::Tree,
    /// OperationId -> (BlockId, index_in_block: usize)
    op_to_block: sled::Tree,
    /// Address -> HashSet<OperationId>
    addr_to_op: sled::Tree,
    /// Address -> HashSet<BlockId>
    addr_to_block: sled::Tree,
    notify: Arc<Notify>,
}

impl BlockStorage {
    pub fn open(
        hash_to_block: sled::Tree,
        slot_to_hash: sled::Tree,
        op_to_block: sled::Tree,
        addr_to_op: sled::Tree, // address -> Vec<operationId>
        addr_to_block: sled::Tree,
        block_count: Arc<AtomicUsize>,
        notify: Arc<Notify>,
    ) -> Result<BlockStorage, StorageError> {
        let res = BlockStorage {
            block_count,
            hash_to_block,
            slot_to_hash,
            op_to_block,
            addr_to_op, // address -> Vec<operationId>
            addr_to_block,
            notify,
        };

        Ok(res)
    }

    pub async fn add_block(&self, block_id: BlockId, block: Block) -> Result<(), StorageError> {
        //acquire W lock on block_count
        massa_trace!("block_storage.add_block", {"block_id": block_id, "block": block});

        //add the new block
        if self.add_block_internal(block_id, block).await? {
            self.block_count.fetch_add(1, Ordering::Release);
            self.notify.notify_one();
        };

        Ok(())
    }

    pub async fn add_block_batch(
        &self,
        blocks: HashMap<BlockId, Block>,
    ) -> Result<(), StorageError> {
        let mut newly_added = 0;

        //add the new blocks
        for (block_id, block) in blocks.into_iter() {
            massa_trace!("block_storage.add_block_batch", {"block_id": block_id, "block": block});
            if self.add_block_internal(block_id, block).await? {
                newly_added += 1;
            };
        }

        if newly_added > 0 {
            self.block_count.fetch_add(newly_added, Ordering::Release);
            self.notify.notify_one();
        }

        Ok(())
    }

    /// Returns a boolean indicating whether the block was a new addition(true) or a replacement(false).
    async fn add_block_internal(
        &self,
        block_id: BlockId,
        block: Block,
    ) -> Result<bool, StorageError> {
        //add the new block
        (
            &self.hash_to_block,
            &self.slot_to_hash,
            &self.op_to_block,
            &self.addr_to_op,
            &self.addr_to_block,
        )
            .transaction(|(hash_tx, slot_tx, op_tx, addr_tx, addr_to_block)| {
                // add block
                let serialized_block = block.to_bytes_compact().map_err(|err| {
                    sled::transaction::ConflictableTransactionError::Abort(
                        InternalError::TransactionError(format!(
                            "error serializing block: {:?}",
                            err
                        )),
                    )
                })?;
                let s_block_id = block_id.to_bytes();
                if hash_tx
                    .insert(&s_block_id, serialized_block.as_slice())?
                    .is_some()
                {
                    return Ok(false);
                }

                let fee_target =
                    Address::from_public_key(&block.header.content.creator).map_err(|err| {
                        sled::transaction::ConflictableTransactionError::Abort(
                            InternalError::TransactionError(format!(
                                "error computing fee target address: {:?}",
                                err
                            )),
                        )
                    })?;

                // slot
                slot_tx.insert(&block.header.content.slot.to_bytes_key(), &s_block_id)?;

                // operations
                let mut addr_to_ops: HashMap<Address, HashSet<OperationId>> = HashMap::new();
                for (idx, op) in block.operations.iter().enumerate() {
                    let op_id = op.get_operation_id().map_err(|err| {
                        sled::transaction::ConflictableTransactionError::Abort(
                            InternalError::TransactionError(format!(
                                "error getting op id block: {:?}",
                                err
                            )),
                        )
                    })?;
                    let s_op_id = op_id.to_bytes();
                    // add to ops
                    op_tx.insert(
                        &s_op_id,
                        [&block_id.to_bytes()[..], &(idx as u64).to_be_bytes()[..]].concat(),
                    )?;
                    // add to involved addrs
                    let involved_addrs = op
                        .get_ledger_involved_addresses(Some(fee_target))
                        .map_err(|err| {
                            sled::transaction::ConflictableTransactionError::Abort(
                                InternalError::TransactionError(format!(
                                    "error getting involved addrs: {:?}",
                                    err
                                )),
                            )
                        })?;
                    for involved_addr in involved_addrs.into_iter() {
                        // get current ops
                        match addr_to_ops.entry(involved_addr) {
                            hash_map::Entry::Occupied(mut occ) => {
                                occ.get_mut().insert(op_id);
                            }
                            hash_map::Entry::Vacant(vac) => {
                                let mut cur_ops =
                                    if let Some(ivec) = addr_tx.get(&involved_addr.to_bytes())? {
                                        ops_from_ivec(ivec).map_err(|err| {
                                            sled::transaction::ConflictableTransactionError::Abort(
                                                InternalError::TransactionError(format!(
                                                    "error getting involved addrs: {:?}",
                                                    err
                                                )),
                                            )
                                        })?
                                    } else {
                                        HashSet::new()
                                    };
                                cur_ops.insert(op_id);
                                vac.insert(cur_ops);
                            }
                        }
                    }
                }
                // write back addr_to_op_cache
                for (addr, op_ids) in addr_to_ops.into_iter() {
                    let ivec = ops_to_ivec(&op_ids).map_err(|err| {
                        sled::transaction::ConflictableTransactionError::Abort(
                            InternalError::TransactionError(format!(
                                "error serializing operation IDs: {:?}",
                                err
                            )),
                        )
                    })?;
                    addr_tx.insert(&addr.to_bytes(), ivec)?;
                }

                // update addr_to_block
                let creator_addr = Address::from_public_key(&block.header.content.creator)
                    .map_err(|err| {
                        sled::transaction::ConflictableTransactionError::Abort(
                            InternalError::TransactionError(format!(
                                "error computing creator address: {:?}",
                                err
                            )),
                        )
                    })?;
                let mut ids =
                    if let Some(ivec_blocks) = addr_to_block.get(&creator_addr.to_bytes())? {
                        block_ids_from_ivec(ivec_blocks).map_err(|err| {
                            sled::transaction::ConflictableTransactionError::Abort(
                                InternalError::TransactionError(format!(
                                    "error deserializing block ids: {:?}",
                                    err
                                )),
                            )
                        })?
                    } else {
                        HashSet::new()
                    };

                ids.insert(block.header.compute_block_id().map_err(|err| {
                    sled::transaction::ConflictableTransactionError::Abort(
                        InternalError::TransactionError(format!(
                            "error computing block id: {:?}",
                            err
                        )),
                    )
                })?);

                let ivec = block_id_to_ivec(&ids).map_err(|err| {
                    sled::transaction::ConflictableTransactionError::Abort(
                        InternalError::TransactionError(format!(
                            "error serializing block ids: {:?}",
                            err
                        )),
                    )
                })?;
                addr_to_block.insert(&creator_addr.to_bytes(), ivec)?;
                Ok(true)
            })
            .map_err(|err| StorageError::AddBlockError(format!("Error adding a block: {:?}", err)))
    }

    pub async fn len(&self) -> Result<usize, StorageError> {
        // Note: we're not synchronizing this across clearing/pruning,
        // so we might as well keep the ordering relaxed.
        Ok(self.block_count.load(Ordering::Relaxed))
    }

    pub async fn contains(&self, block_id: BlockId) -> Result<bool, StorageError> {
        self.hash_to_block
            .contains_key(block_id.to_bytes())
            .map_err(StorageError::from)
    }

    pub fn get_block(&self, block_id: BlockId) -> Result<Option<Block>, StorageError> {
        massa_trace!("block_storage.get_block", { "block_id": block_id });
        let hash_key = block_id.to_bytes();
        if let Some(s_block) = self.hash_to_block.get(hash_key)? {
            Ok(Some(Block::from_bytes_compact(s_block.as_ref())?.0))
        } else {
            Ok(None)
        }
    }

    pub async fn get_slot_range(
        &self,
        start: Option<Slot>,
        end: Option<Slot>,
    ) -> Result<HashMap<BlockId, Block>, StorageError> {
        let start_key = start.map(|v| v.to_bytes_key());
        let end_key = end.map(|v| v.to_bytes_key());

        match (start_key, end_key) {
            (None, None) => self.slot_to_hash.iter(),
            (Some(b1), None) => self.slot_to_hash.range(b1..),
            (None, Some(b2)) => self.slot_to_hash.range(..b2),
            (Some(b1), Some(b2)) => self.slot_to_hash.range(b1..b2),
        }
        .filter_map(|item| {
            let (_, s_hash) = item.ok()?;
            let hash = BlockId::from_bytes(
                &s_hash
                    .as_ref()
                    .try_into()
                    .map_err(|err| {
                        StorageError::DeserializationError(format!(
                            "wrong buffer size for hash deserialization: {:?}",
                            err
                        ))
                    })
                    .ok()?,
            )
            .ok()?;

            // Note: blocks not found are ignored.
            match self.get_block(hash) {
                Ok(Some(block)) => Some(Ok((hash, block))),
                _ => None,
            }
        })
        .collect::<Result<HashMap<BlockId, Block>, StorageError>>()
    }

    pub async fn get_operations(
        &self,
        operation_ids: HashSet<OperationId>,
    ) -> Result<HashMap<OperationId, OperationSearchResult>, StorageError> {
        let tx_func =
            |op_tx: &TransactionalTree,
             hash_tx: &TransactionalTree|
             -> Result<HashMap<OperationId, OperationSearchResult>, StorageError> {
                let mut res: HashMap<OperationId, OperationSearchResult> = HashMap::new();

                for id in operation_ids.iter() {
                    let ser_op_id = id.to_bytes();
                    let (block_id, idx) = if let Some(buf) = op_tx.get(&ser_op_id)? {
                        let block_id = BlockId::from_bytes(&array_from_slice(&buf[0..])?)?;
                        let idx: usize =
                            u64::from_be_bytes(array_from_slice(&buf[BLOCK_ID_SIZE_BYTES..])?)
                                as usize;
                        (block_id, idx)
                    } else {
                        continue;
                    };
                    if let Some(s_block) = hash_tx.get(&block_id.to_bytes())? {
                        let (mut block, _size) = Block::from_bytes_compact(s_block.as_ref())?;
                        if idx >= block.operations.len() {
                            return Err(StorageError::DatabaseInconsistency(
                                "operation index overflows block operations length".into(),
                            ));
                        }
                        res.insert(
                            *id,
                            OperationSearchResult {
                                op: block.operations.swap_remove(idx),
                                in_pool: false,
                                in_blocks: vec![(block_id, (idx, true))].into_iter().collect(),
                                status: OperationSearchResultStatus::InBlock(
                                    OperationSearchResultBlockStatus::Stored,
                                ),
                            },
                        );
                    } else {
                        return Err(StorageError::DatabaseInconsistency(
                            "could not find a block referenced by operation index".into(),
                        ));
                    }
                }

                Ok(res)
            };

        (&self.op_to_block, &self.hash_to_block)
            .transaction(|(op_tx, hash_tx)| {
                tx_func(op_tx, hash_tx).map_err(|err| {
                    sled::transaction::ConflictableTransactionError::Abort(
                        InternalError::TransactionError(format!("transaction error: {:?}", err)),
                    )
                })
            })
            .map_err(|err| {
                StorageError::OperationError(format!("error getting operation: {:?}", err))
            })
    }

    pub async fn get_operations_involving_address(
        &self,
        address: &Address,
    ) -> Result<HashMap<OperationId, OperationSearchResult>, StorageError> {
        let tx_func =
            |addr_to_op: &TransactionalTree,
             op_to_block: &TransactionalTree,
             hash_to_block: &TransactionalTree|
             -> Result<HashMap<OperationId, OperationSearchResult>, StorageError> {
                if let Some(ops) = addr_to_op.get(address.to_bytes())? {
                    Ok(ops_from_ivec(ops)?
                        .into_iter()
                        .map(|id| {
                            let ser_op_id = id.to_bytes();
                            let (block_id, idx) = if let Some(buf) = op_to_block.get(&ser_op_id)? {
                                let block_id = BlockId::from_bytes(&array_from_slice(&buf[0..])?)?;
                                let idx: usize = u64::from_be_bytes(array_from_slice(
                                    &buf[BLOCK_ID_SIZE_BYTES..],
                                )?) as usize;
                                (block_id, idx)
                            } else {
                                return Err(StorageError::DatabaseInconsistency(
                                    "inconsistency between addr to op and op to block".to_string(),
                                ));
                            };
                            let ser_block_id = block_id.to_bytes();
                            let block = if let Some(s_block) = hash_to_block.get(ser_block_id)? {
                                Block::from_bytes_compact(s_block.as_ref())?.0
                            } else {
                                return Err(StorageError::DatabaseInconsistency(
                                    "Inconsistency between op to block and hash to block"
                                        .to_string(),
                                ));
                            };

                            Ok((
                                id,
                                OperationSearchResult {
                                    op: block.operations[idx].clone(),
                                    in_pool: false,
                                    in_blocks: vec![(block_id, (idx, true))].into_iter().collect(),
                                    status: OperationSearchResultStatus::InBlock(
                                        OperationSearchResultBlockStatus::Stored,
                                    ),
                                },
                            ))
                        })
                        .collect::<Result<_, _>>()?)
                } else {
                    Ok(HashMap::new())
                }
            };

        (&self.addr_to_op, &self.op_to_block, &self.hash_to_block)
            .transaction(|(addr_tx, op_tx, hash_tx)| {
                tx_func(addr_tx, op_tx, hash_tx).map_err(|err| {
                    sled::transaction::ConflictableTransactionError::Abort(
                        InternalError::TransactionError(format!("transaction error: {:?}", err)),
                    )
                })
            })
            .map_err(|err| {
                StorageError::OperationError(format!("error getting operation: {:?}", err))
            })
    }

    pub async fn get_block_ids_by_creator(
        &self,
        address: &Address,
    ) -> Result<HashSet<BlockId>, StorageError> {
        Ok(self.addr_to_block.transaction(|addr_to_block| {
            if let Some(ivec_blocks) = addr_to_block.get(&address.to_bytes())? {
                Ok(block_ids_from_ivec(ivec_blocks).map_err(|err| {
                    sled::transaction::ConflictableTransactionError::Abort(
                        InternalError::TransactionError(format!(
                            "error deserializing block ids: {:?}",
                            err
                        )),
                    )
                })?)
            } else {
                Ok(HashSet::with_capacity(0))
            }
        })?)
    }
}
