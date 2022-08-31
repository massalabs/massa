//! Copyright (c) 2022 MASSA LABS <info@massa.net>

use crate::{
    node_info::NodeInfo, protocol_worker::BlockRetrievalStatus, protocol_worker::ProtocolWorker,
    worker_operations_impl::OperationBatchBuffer,
};

use massa_logging::massa_trace;

use massa_models::block::{self, Block};
use massa_models::cache::{HashCacheMap, HashCacheSet};
use massa_models::endorsement::Endorsement;
use massa_models::error::ModelsError;
use massa_models::{
    block::{BlockId, WrappedHeader},
    endorsement::{EndorsementId, WrappedEndorsement},
    node::NodeId,
    operation::OperationPrefixId,
    operation::{OperationId, WrappedOperation},
    prehash::{CapacityAllocator, PreHashMap, PreHashSet},
};
use massa_network_exports::{AskForBlocksInfo, NetworkCommandSender, NetworkEventReceiver};
use massa_pool_exports::PoolController;
use massa_protocol_exports::{
    ProtocolCommand, ProtocolCommandSender, ProtocolConfig, ProtocolError, ProtocolEvent,
    ProtocolEventReceiver, ProtocolManagementCommand, ProtocolManager,
};

use massa_storage::Storage;
use massa_time::TimeError;
use rand::{thread_rng, Rng};
use std::collections::{hash_map, HashMap, HashSet, VecDeque};
use tokio::{
    sync::mpsc,
    sync::mpsc::error::SendTimeoutError,
    time::{sleep, sleep_until, Instant, Sleep},
};
use tracing::{debug, error, info, warn};

impl ProtocolWorker {
    /// Update the status of the block retrieval process
    pub(crate) async fn update_ask_block(
        &mut self,
        ask_block_timer: &mut std::pin::Pin<&mut Sleep>,
    ) -> Result<(), ProtocolError> {
        massa_trace!("protocol.protocol_worker.update_ask_block.begin", {});

        let now = Instant::now();

        // init timer
        let mut next_tick = now
            .checked_add(self.config.ask_block_timeout.into())
            .ok_or(TimeError::TimeOverflowError)?;

        // cleanup failed ask list
        while let Some((_, _, timestamp)) = self.failed_block_asks.front() {
            let expiry = timestamp
                .checked_add(self.config.ask_block_retry_time.to_duration())
                .ok_or(TimeError::TimeOverflowError)?;
            if now >= expiry {
                self.failed_block_asks.pop_front();
            } else {
                // update next tick
                next_tick = std::cmp::min(next_tick, expiry);
                break;
            }
        }

        // send finished retrievals to consensus
        let to_finalize: Vec<BlockId> = self
            .block_wishlist
            .iter()
            .filter_map(|(id, status)| {
                if let BlockRetrievalStatus::FullyRetrieved { .. } = status {
                    return Some(id);
                }
                None
            })
            .collect();
        to_finalize.into_iter().for_each(
            |block_id| {
                while self.run_block_retrieval_step(&block_id, None) {}
            },
        );

        // abort if there is nothing to do
        if self.active_nodes.is_empty() || self.block_wishlist.is_empty() {
            // no need to do anything: reset timer and quit
            ask_block_timer.set(sleep_until(next_tick));
            return Ok(());
        }

        // check active block queries for timeout to gather needed queries
        // also accumulate node load (prioritary and non-prioritary)
        // also detect next timeout
        let mut node_load: HashMap<NodeId, usize> =
            self.active_nodes.keys().map(|k| (*k, 0)).collect();
        let mut needed_queries: PreHashSet<BlockId> = PreHashSet::new();
        for block_id in self.block_wishlist.keys().copied().collect::<Vec<_>>() {
            let (timestamp, node_id) = match self.block_wishlist.get(&block_id).unwrap() {
                BlockRetrievalStatus::FullyRetrieved { .. } => {} // not needed
                BlockRetrievalStatus::InvalidBlock { .. } => {}   // not needed
                BlockRetrievalStatus::Input { .. } => {
                    // A query is needed, but no node is currently being asked,
                    // so we skip timeout/disconnect detection.
                    needed_queries.insert(block_id);
                    continue;
                }
                running => running.get_query_info().unwrap(),
            };
            // check for timeout or node disconnect
            let epxpires_at = timestamp
                .checked_add(self.config.ask_block_timeout.into())
                .ok_or(TimeError::TimeOverflowError)?;
            if now >= epxpires_at || !self.active_nodes.contains_key(&queried_node) {
                // add to failed list
                self.failed_block_asks
                    .push_back((block_id, queried_node, now));
                // needs a new query
                needed_queries.insert(block_id);
                // mark the node as not having the block
                if let Some(node_info) = self.active_nodes.get_mut(&queried_node) {
                    node_info.known_blocks.remove(&block_id);
                }
            } else {
                // count node load
                let load = node_load.entry(node_id).or_default();
                *load = load.saturating_add(1);
                // compute next tick
                next_tick = std::cmp::min(next_tick, epxpires_at);
            }
        }
        // filter out overloaded nodes
        node_load.retain(|(_, load)| load < self.config.max_simultaneous_ask_blocks_per_node);

        // no needed queries or no nodes available
        if needed_queries.is_empty() || node_load.is_empty() {
            // no need to do anything: reset timer and quit
            ask_block_timer.set(sleep_until(next_tick));
            return Ok(());
        }
        let needed_queries: Vec<_> = needed_queries.into_iter().collect();

        // list recently failed asks
        let mut recent_failed_asks: PreHashMap<BlockId, (Option<Instant>, HashSet<NodeId>)> =
            needed_queries
                .iter()
                .map(|b| (*b, Default::default()))
                .collect();
        self.failed_block_asks.iter().forEach(|(b, n, t)| {
            let (res_t, res_ns) = recent_failed_asks.get_mut(b).unwrap();
            *res_t = std::cmp::max(*res_t, *t);
            res_ns.push(*n);
        });

        // sort queries starting from highest priority (in wishlist), then from earliest failure. Shuffle on equality.
        thread_rng().shuffle(&mut needed_queries);
        // use stable sort here to keep the shuffling
        needed_queries.sort_by_cached_key(|b| {
            let not_in_wishlist = !self.block_wishlist.contains_key(b);
            let last_fail = recent_failed_asks[b].0.clone();
            (not_in_wishlist, last_fail)
        });

        // generate query requests
        for block_id in needed_queries {
            // choose the best node by block knowledge, then lowest load, then oldest (ideally no) failure
            let (node_id, load) = match node_load
                .iter_mut()
                // filter out overloaded nodes
                .filter(|(node_id, load)| {
                    load <= self.config.max_simultaneous_ask_blocks_per_node
                        && !recent_failed_asks[&block_id].1.contains(node_id)
                })
                .min_by_key(|(node_id, load)| {
                    let does_not_know_block = !self
                        .active_nodes
                        .get(node_id)
                        .unwrap()
                        .known_blocks
                        .contains(&block_id);
                    let last_fail = recent_failed_asks[&block_id].0.clone();
                    (does_not_know_block, *load, last_fail)
                }) {
                Some(v) => v,     // return best suitable node
                None => continue, // no suitable node found to get this block
            };
            // increment load
            *load = load.saturating_add(1);
            // run a block retrieval step for that block ID
            while self.run_block_retrieval_step(&block_id, Some(*node_id)) {}
        }

        // reset timer
        ask_block_timer.set(sleep_until(next_tick));

        Ok(())
    }

    /// Run a step of block retrieval
    /// Returns true if the state changed
    async fn run_block_retrieval_step(
        &mut self,
        block_id: &BlockId,
        node_id: Option<NodeId>,
    ) -> bool {
        let timestamp = Instant::now();

        // remove from wishlist
        let mut status = match self.block_wishlist.remove(block_id) {
            Some(v) => v,
            None => return,
        };

        let mut state_changed = false;
        let status = match status {
            // the retrieval process hasn't started yet: start it
            BlockRetrievalStatus::Input { opt_header } => {
                let queried_node =
                    node_id.expect("A node ID is required for Input block retrieval status");
                state_changed = true;
                match opt_header {
                    // we don't have the header: ask for it
                    None => Some(BlockRetrievalStatus::QueryingHeader {
                        queried_node,
                        timestamp,
                    }),
                    // we already have the header: directly ask for block operation list
                    Some(header) => Some(BlockRetrievalStatus::QueryingOpList {
                        header,
                        block_storage,
                        queried_node,
                        timestamp,
                    }),
                }
            }

            // query a header
            BlockRetrievalStatus::QueryingHeader { .. } => {
                let queried_node = node_id
                    .expect("A node ID is required for QueryingHeader block retrieval status");
                let _ = self
                    .network_command_sender
                    .ask_for_header(block_id, queried_node)
                    .await;
                Some(BlockRetrievalStatus::QueryingHeader {
                    queried_node,
                    timestamp,
                })
            }

            // query block operation list
            BlockRetrievalStatus::QueryingOpList {
                header,
                block_storage,
                ..
            } => {
                let queried_node = node_id
                    .expect("A node ID is required for QueryingOpList block retrieval status");
                let _ = self
                    .network_command_sender
                    .ask_for_block_ops_list(block_id, queried_node)
                    .await;
                Some(BlockRetrievalStatus::QueryingOpList {
                    header,
                    block_storage,
                    queried_node,
                    timestamp,
                })
            }

            // query missing block operations
            BlockRetrievalStatus::QueryingOps {
                header,
                operation_ids,
                block_storage,
                ..
            } => {
                // attempt to claim operations to storage
                let full_ops_set: PreHashSet<_> = operation_ids.iter().copied().collect();
                let claimed_ops = block_storage.claim_operation_refs(&full_ops_set);
                let missing_ops: HashSet<_> = full_ops_set - claimed_ops;
                if missing_ops.is_empty() {
                    // no operations missing => build block
                    state_changed = true;
                    let slot = header.content.slot;
                    match self.try_build_retrieved_block(header, operation_ids, &mut block_storage)
                    {
                        Ok(()) => Some(BlockRetrievalStatus::FullyRetrieved {
                            slot,
                            block_storage,
                        }),
                        Err(err) => Some(BlockRetrievalStatus::InvalidBlock {
                            reason: err.to_string(),
                        }),
                    }
                } else {
                    // some ops are still missing => ask for them
                    let queried_node = node_id
                        .expect("A node ID is required for QueryingOps block retrieval status");
                    let _ = self
                        .network_command_sender
                        .ask_for_block_ops(block_id, queried_node, &missing)
                        .await;
                    Some(BlockRetrievalStatus::QueryingOps {
                        header,
                        operation_ids,
                        block_storage,
                        queried_node,
                        timestamp,
                    })
                }
            }

            // the block is fully retrieved: send it as an event
            BlockRetrievalStatus::FullyRetrieved {
                slot,
                block_storage,
            } => {
                match self
                    .send_protocol_event(ProtocolEvent::ReceivedBlock {
                        block_id: *block_id,
                        slot,
                        storage: block_storage,
                    })
                    .await
                {
                    Ok(()) => None, // successfully sent: don't add back to wishlist
                    Err(event) => {
                        // on event send failure, reconstruct the status
                        Some(BlockRetrievalStatus::FullyRetrieved {
                            slot: event.slot,
                            block_storage: event.storage,
                        })
                    }
                }
            }

            // the block appeared as invalid during retrieval (for example due to being too big)
            BlockRetrievalStatus::InvalidBlock { reason } => {
                match self
                    .send_protocol_event(ProtocolEvent::ReceivedInvalidBlock {
                        block_id: *block_id,
                        reason,
                    })
                    .await
                {
                    Ok(()) => None, // successfully sent: don't add back to wishlist
                    Err(event) => {
                        // on event send failure, reconstruct the status
                        Some(BlockRetrievalStatus::InvalidBlock {
                            reason: event,
                            reason,
                        })
                    }
                }
            }
        };

        // update status if needed
        if let Some(status) = status {
            self.block_wishlist.insert(*block_id, status);
        }

        state_changed
    }

    /// a node header was received
    async fn on_block_header_received(
        &mut self,
        node_id: &NodeId,
        mut header: WrappedHeader,
    ) -> Result<(), ProtocolError> {
        let timestamp = Instant::now();
        let block_id = header.id;

        let is_new;
        match self.check_header(&header) {
            Ok(Some(cached)) => {
                header = cached; // replace with cached verion to prevent malleability attacks
                is_new = false;
            }
            Ok(None) => {
                id_new = true;
            }
            Err(ProtocolError::WrongSignature) => {
                // wrong sig: ignore object
                let _ = self.ban_node(node_id).await;
                return Ok(());
            }
            Err(err) => {
                // invalid object
                if let Some(status) = self.block_wishlist.get_mut(&block_id) {
                    // it was in wishist: mark it as invalid
                    *status = BlockRetrievalStatus::InvalidBlock {
                        reason: format!("invalid header: {}", err),
                    };
                }
                let _ = self.ban_node(node_id).await;
                return Ok(());
            }
        }

        // mark sender as knowing the block and endorsements
        if Some(node_info) = self.active_nodes.get_mut(node_id) {
            node_info.known_blocks.insert(*block_id);
            header
                .content
                .endorsements
                .iter()
                .for_each(|endo| node_info.knwon_endorsements.insert(*endo.id));
        }

        let status = match self.block_wishlist.remove(&block_id) {
            None => {
                // pure announcement header, not in wishlist
                if !is_new {
                    // not new: ignore
                    return Ok(());
                }
                // send to consensus
                self.network_command_sender
                    .send_block_header(*node_id, header)
                    .await?;
                None
            }
            // if haven't started asking yet, or if we were currently asking for that header
            Some(BlockRetrievalStatus::Input { .. })
            | Some(BlockRetrievalStatus::QueryingHeader { .. }) => {
                // we need to query the sender
                let mut block_storage = self.storage.clone_without_refs();
                block_storage.store_endorsements(header.content.endorsements.clone());
                Some(BlockRetrievalStatus::QueryingOpList {
                    header,
                    block_storage,
                    queried_node: *node_id,
                    timestamp,
                })
            }
            // otherwise, keep current status
            Some(other) => Some(other),
        };

        // update status if needed
        if let Some(status) = status {
            self.block_wishlist.insert(*block_id, status);
        }

        // run block retrieval step
        self.update_ask_block().await?;

        Ok(())
    }

    // we just received a block operation list
    async fn on_block_ops_list_received(
        &mut self,
        node_id: &NodeId,
        block_id: &BlockId,
        operation_ids: Vec<OperationId>,
    ) {
        // check ops length
        if operation_ids.len() > self.config.max_block_size {
            // invalid ops length
            let _ = self.ban_node(node_id).await;
            return Ok(());
        }

        // check if we need this value
        let header = match self.block_wishlist.get(&block_id) {
            // we haven't started asking yet but have the header
            Some(BlockRetrievalStatus::Input {
                opt_header: Some(header),
            }) => header,
            // we were currently asking for the op ID list
            Some(BlockRetrievalStatus::QueryingOpList { header, .. }) => header,
            // otherwise, keep the current status if any
            other_status => {
                self.update_ask_block().await?;
                return Ok(());
            }
        };

        // check ops hash
        if header.content.operation_merkle_root
            != Hash::compute_from(
                &self
                    .operations
                    .iter()
                    .flat_map(|op| op.id.into_bytes())
                    .collect::<Vec<_>>()[..],
            )
        {
            // invalid ops hash
            let _ = self.ban_node(node_id).await;
            return Ok(());
        }

        // remove entry from wishlist and gather header and storage
        let (header, block_storage) = match self.block_wishlist.remove(&block_id) {
            // we haven't started asking yet but have the header
            Some(BlockRetrievalStatus::Input {
                opt_header: Some(header),
            }) => {
                // create storage
                let mut block_storage = self.storage.clone_without_refs();
                block_storage.store_endorsements(header.content.endorsements.clone());
                (header, block_storage)
            }
            // we were currently asking for the op ID list
            Some(BlockRetrievalStatus::QueryingOpList {
                header,
                block_storage,
                ..
            }) => (header, block_storage),
            // cannot be anything else (checked above)
            other_status => panic!("unexpected wishlist status"),
        };

        // claim the ops, list the ones missing
        let all_ops: PreHashSet<_> = ops.iter().copied().collect();
        let claimed_ops = block_storage.claim_operation_refs(&all_ops);
        let missing_ops: PreHashSet<_> = all_ops - claimed_ops;

        if missing_ops.is_empty() {
            // there are no missing ops => build full block
            let slot = header.content.slot;
            let status =
                match self.try_build_retrieved_block(header, operation_ids, &mut block_storage) {
                    Ok(()) => Some(BlockRetrievalStatus::FullyRetrieved {
                        slot,
                        block_storage,
                    }),
                    Err(err) => Some(BlockRetrievalStatus::InvalidBlock {
                        reason: err.to_string(),
                    }),
                };
            self.block_wishlist.insert(*block_id, status);
            self.update_ask_block().await?;
            return Ok(());
        }

        // ask missing blocks
        let timestamp = Instant::now();
        self.block_wishlist.insert(
            *block_id,
            BlockRetrievalStatus::QueryingOps {
                header,
                operation_ids,
                block_storage,
                queried_node: *node_id,
                timestamp,
            },
        );

        // run block retrieval step
        self.update_ask_block().await?;

        Ok(())
    }

    /// Checks a header validity.
    /// Returns the cached version if the header was in cache before.
    fn check_header(
        &mut self,
        header: &WrappedHeader,
    ) -> Result<Option<WrappedHeader>, ProtocolError> {
        // Check if the verification was already done in the past
        if let Some(prev_header) = self.checked_header_sigs.get(&block_id) {
            // Return that the check was successful
            return Ok(Some(prev_header.clone()));
        }

        // Check signature
        if header.verify_signature().is_err() {
            return ProtocolError::WrongSignature;
        }

        // Reject genesis
        if (header.content.slot.period == 0) || header.content.parents.is_empty() {
            return Err(ProtocolError::ModelsError(ModelsError::DeserializeError(
                "genesis blocks forbidden in protocol".into(),
            )));
        }

        // Verify endorsements
        {
            let parent_in_own_thread = header.content.parents[header.content.slot.thread as usize];
            let mut endo_indices = HashSet::with_capacity(header.content.endorsements.len());
            for endo in &header.content.endorsements {
                if endo.content.endorsed_block != parent_in_own_thread {
                    return Err(ProtocolError::ModelsError(ModelsError::DeserializeError(
                        "endorsement targeting wrong block".into(),
                    )));
                }
                if !endo_indices.insert(endo.content.index) {
                    return Err(ProtocolError::ModelsError(ModelsError::DeserializeError(
                        "duplicate endorsement index".into(),
                    )));
                }
                // check endorsement validity
                self.check_endorsement(endo).map_err(|err| {
                    // TODO this check should be STRICT for malleability for now
                    ModelsError::DeserializeError(format!("invalid endorsement: {}", err))
                })?;
            }
        }

        // Add to cache
        self.checked_headers.insert(block_id, header.clone());
        Ok(None)
    }
}
