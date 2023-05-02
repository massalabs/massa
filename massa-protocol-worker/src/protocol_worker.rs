//! Copyright (c) 2022 MASSA LABS <info@massa.net>

use crate::cache::{LinearHashCacheMap, LinearHashCacheSet};
use crate::checked_operations::CheckedOperations;
use crate::sig_verifier::verify_sigs_batch;
use crate::{node_info::NodeInfo, worker_operations_impl::OperationBatchBuffer};

use massa_consensus_exports::ConsensusController;
use massa_logging::massa_trace;

use massa_models::secure_share::Id;
use massa_models::slot::Slot;
use massa_models::timeslots::get_block_slot_timestamp;
use massa_models::{
    block_header::SecuredHeader,
    block_id::BlockId,
    endorsement::{EndorsementId, SecureShareEndorsement},
    node::NodeId,
    operation::OperationPrefixId,
    operation::{OperationId, SecureShareOperation},
    prehash::{CapacityAllocator, PreHashMap, PreHashSet},
};
use massa_network_exports::{AskForBlocksInfo, NetworkCommandSender, NetworkEventReceiver};
use massa_pool_exports::PoolController;
use massa_protocol_exports::{
    ProtocolCommand, ProtocolConfig, ProtocolError, ProtocolManagementCommand, ProtocolManager,
    ProtocolReceivers, ProtocolSenders,
};
use massa_storage::Storage;
use massa_time::{MassaTime, TimeError};
use std::collections::{HashMap, HashSet};
use std::mem;
use std::pin::Pin;
use tokio::{
    sync::mpsc,
    time::{sleep, sleep_until, Instant, Sleep},
};
use tracing::{debug, error, info, warn};

/// start a new `ProtocolController` from a `ProtocolConfig`
/// - generate keypair
/// - create `protocol_command/protocol_event` channels
/// - launch `protocol_controller_fn` in an other task
///
/// # Arguments
/// * `config`: protocol settings
/// * `senders`: sender(s) channel(s) to communicate with other modules
/// * `receivers`: receiver(s) channel(s) to communicate with other modules
/// * `consensus_controller`: interact with consensus module
/// * `storage`: Shared storage to fetch data that are fetch across all modules
pub async fn start_protocol_controller(
    config: ProtocolConfig,
    receivers: ProtocolReceivers,
    senders: ProtocolSenders,
    consensus_controller: Box<dyn ConsensusController>,
    pool_controller: Box<dyn PoolController>,
    storage: Storage,
) -> Result<ProtocolManager, ProtocolError> {
    debug!("starting protocol controller");

    // launch worker
    let (manager_tx, controller_manager_rx) = mpsc::channel::<ProtocolManagementCommand>(1);
    let pool_controller = pool_controller.clone();
    let join_handle = tokio::spawn(async move {
        let res = ProtocolWorker::new(
            config,
            ProtocolWorkerChannels {
                network_command_sender: senders.network_command_sender,
                network_event_receiver: receivers.network_event_receiver,
                controller_command_rx: receivers.protocol_command_receiver,
                controller_manager_rx,
            },
            consensus_controller,
            pool_controller,
            storage,
        )
        .run_loop()
        .await;
        match res {
            Err(err) => {
                error!("protocol worker crashed: {}", err);
                Err(err)
            }
            Ok(v) => {
                info!("protocol worker finished cleanly");
                Ok(v)
            }
        }
    });
    debug!("protocol controller ready");
    Ok(ProtocolManager::new(join_handle, manager_tx))
}

/// Info about a block we've seen
#[derive(Debug, Clone)]
pub(crate) struct BlockInfo {
    /// The header of the block.
    pub(crate) header: Option<SecuredHeader>,
    /// Operations ids. None if not received yet
    pub(crate) operation_ids: Option<Vec<OperationId>>,
    /// Operations and endorsements contained in the block,
    /// if we've received them already, and none otherwise.
    pub(crate) storage: Storage,
    /// Full operations size in bytes
    pub(crate) operations_size: usize,
}

impl BlockInfo {
    fn new(header: Option<SecuredHeader>, storage: Storage) -> Self {
        BlockInfo {
            header,
            operation_ids: None,
            storage,
            operations_size: 0,
        }
    }
}

/// protocol worker
pub struct ProtocolWorker {
    /// Protocol configuration.
    pub(crate) config: ProtocolConfig,
    /// Consensus controller
    pub(crate) consensus_controller: Box<dyn ConsensusController>,
    /// Associated network command sender.
    pub(crate) network_command_sender: NetworkCommandSender,
    /// Associated network event receiver.
    network_event_receiver: NetworkEventReceiver,
    /// Channel to send protocol pool events to the controller.
    pool_controller: Box<dyn PoolController>,
    /// Channel receiving commands from the controller.
    controller_command_rx: mpsc::Receiver<ProtocolCommand>,
    /// Channel to send management commands to the controller.
    controller_manager_rx: mpsc::Receiver<ProtocolManagementCommand>,
    /// Ids of active nodes mapped to node info.
    pub(crate) active_nodes: HashMap<NodeId, NodeInfo>,
    /// List of wanted blocks,
    /// with the info representing their state with in the `as_block` workflow.
    pub(crate) block_wishlist: PreHashMap<BlockId, BlockInfo>,
    /// List of processed endorsements
    checked_endorsements: LinearHashCacheSet<EndorsementId>,
    /// Cache of processed operations
    pub(crate) checked_operations: CheckedOperations,
    /// List of processed headers
    pub(crate) checked_headers: LinearHashCacheMap<BlockId, SecuredHeader>,
    /// List of ids of operations that we asked to the nodes
    pub(crate) asked_operations: PreHashMap<OperationPrefixId, (Instant, Vec<NodeId>)>,
    /// Buffer for operations that we want later
    pub(crate) op_batch_buffer: OperationBatchBuffer,
    /// Shared storage.
    pub(crate) storage: Storage,
    /// Operations to announce at the next interval.
    operations_to_announce: Vec<OperationId>,
}

/// channels used by the protocol worker
pub struct ProtocolWorkerChannels {
    /// network command sender
    pub network_command_sender: NetworkCommandSender,
    /// network event receiver
    pub network_event_receiver: NetworkEventReceiver,
    /// protocol command receiver
    pub controller_command_rx: mpsc::Receiver<ProtocolCommand>,
    /// protocol management command receiver
    pub controller_manager_rx: mpsc::Receiver<ProtocolManagementCommand>,
}

impl ProtocolWorker {
    /// Creates a new protocol worker.
    ///
    /// # Arguments
    /// * `config`: protocol configuration.
    /// * `network_controller`: associated network controller.
    /// * `controller_event_tx`: Channel to send protocol events.
    /// * `controller_command_rx`: Channel receiving commands.
    /// * `controller_manager_rx`: Channel receiving management commands.
    pub fn new(
        config: ProtocolConfig,
        ProtocolWorkerChannels {
            network_command_sender,
            network_event_receiver,
            controller_command_rx,
            controller_manager_rx,
        }: ProtocolWorkerChannels,
        consensus_controller: Box<dyn ConsensusController>,
        pool_controller: Box<dyn PoolController>,
        storage: Storage,
    ) -> ProtocolWorker {
        ProtocolWorker {
            config,
            network_command_sender,
            network_event_receiver,
            consensus_controller,
            pool_controller,
            controller_command_rx,
            controller_manager_rx,
            active_nodes: Default::default(),
            block_wishlist: Default::default(),
            checked_endorsements: LinearHashCacheSet::new(config.max_known_endorsements_size),
            checked_operations: CheckedOperations::new(config.max_known_ops_size),
            checked_headers: LinearHashCacheMap::new(config.max_node_known_blocks_size),
            asked_operations: Default::default(),
            op_batch_buffer: OperationBatchBuffer::with_capacity(
                config.operation_batch_buffer_capacity,
            ),
            storage,
            operations_to_announce: Vec::with_capacity(
                config.operation_announcement_buffer_capacity,
            ),
        }
    }

    /// Main protocol worker loop. Consumes self.
    /// It is mostly a `tokio::select!` inside a loop
    /// waiting on :
    /// - `controller_command_rx`
    /// - `network_controller`
    /// - `handshake_futures`
    /// - `node_event_rx`
    /// And at the end every thing is closed properly
    /// Consensus work is managed here.
    /// It's mostly a `tokio::select!` within a loop.
    pub async fn run_loop(mut self) -> Result<NetworkEventReceiver, ProtocolError> {
        // TODO: Config variable for the moment 10000 (prune) (100 seconds)
        let operation_prune_timer = sleep(self.config.asked_operations_pruning_period.into());
        tokio::pin!(operation_prune_timer);
        let block_ask_timer = sleep(self.config.ask_block_timeout.into());
        tokio::pin!(block_ask_timer);
        let operation_batch_proc_period_timer =
            sleep(self.config.operation_batch_proc_period.into());
        tokio::pin!(operation_batch_proc_period_timer);
        let operation_announcement_interval =
            sleep(self.config.operation_announcement_interval.into());
        tokio::pin!(operation_announcement_interval);
        loop {
            massa_trace!("protocol.protocol_worker.run_loop.begin", {});
            /*
                select! without the "biased" modifier will randomly select the 1st branch to check,
                then will check the next ones in the order they are written.
                We choose this order:
                    * manager commands: low freq, avoid having to wait to stop
                    * incoming commands (high frequency): process commands in priority (this is a high-level crate so we prioritize this side to avoid slowing down consensus)
                    * network events (high frequency): process incoming events
                    * ask for blocks (timing not important)
            */
            tokio::select! {
                // listen to management commands
                cmd = self.controller_manager_rx.recv() => {
                    massa_trace!("protocol.protocol_worker.run_loop.controller_manager_rx", { "cmd": cmd });
                    match cmd {
                        None => break,
                        Some(_) => {}
                    };
                }

                // listen to incoming commands
                Some(cmd) = self.controller_command_rx.recv() => {
                    self.process_command(cmd, &mut
                        block_ask_timer,
                        &mut operation_announcement_interval).await?;
                }

                // listen to network controller events
                evt = self.network_event_receiver.wait_event() => {
                    massa_trace!("protocol.protocol_worker.run_loop.network_event_rx", {});
                    self.on_network_event(evt?, &mut block_ask_timer, &mut operation_announcement_interval).await?;
                }

                // block ask timer
                _ = &mut block_ask_timer => {
                    massa_trace!("protocol.protocol_worker.run_loop.block_ask_timer", { });
                    self.update_ask_block(&mut block_ask_timer).await?;
                }

                // Operation announcement interval.
                _ = &mut operation_announcement_interval => {
                    // Announce operations.
                    self.announce_ops(&mut operation_announcement_interval).await;
                }

                // operation ask timer
                _ = &mut operation_batch_proc_period_timer => {
                    massa_trace!("protocol.protocol_worker.run_loop.operation_ask_and_announce_timer", { });

                    // Update operations to ask.
                    self.update_ask_operation(&mut operation_batch_proc_period_timer).await?;
                }
                // operation prune timer
                _ = &mut operation_prune_timer => {
                    massa_trace!("protocol.protocol_worker.run_loop.operation_prune_timer", { });
                    self.prune_asked_operations(&mut operation_prune_timer)?;
                }
            }
            massa_trace!("protocol.protocol_worker.run_loop.end", {});
        }

        Ok(self.network_event_receiver)
    }

    /// Announce a set of operations to active nodes who do not know about it yet.
    /// Side effects:
    /// - notes nodes as knowing about those operations from now on.
    /// - empties the buffer of operations to announce.
    async fn announce_ops(&mut self, timer: &mut Pin<&mut Sleep>) {
        // Quit if empty  to avoid iterating on nodes
        if self.operations_to_announce.is_empty() {
            // Reset timer.
            let now = Instant::now();
            let next_tick = now
                .checked_add(self.config.operation_announcement_interval.into())
                .expect("time overflow");
            timer.set(sleep_until(next_tick));
            return;
        }
        let operation_ids = mem::take(&mut self.operations_to_announce);
        massa_trace!("protocol.protocol_worker.announce_ops.begin", {
            "operation_ids": operation_ids
        });
        for (node, node_info) in self.active_nodes.iter_mut() {
            let new_ops: Vec<OperationId> = operation_ids
                .iter()
                .filter(|id| !node_info.knows_op(&id.prefix()))
                .copied()
                .collect();
            if !new_ops.is_empty() {
                node_info.insert_known_ops(new_ops.iter().map(|id| id.prefix()));

                let res = self
                    .network_command_sender
                    .announce_operations(*node, new_ops.iter().map(|id| id.into_prefix()).collect())
                    .await;
                if let Err(err) = res {
                    debug!("could not send operation batch to node {}: {}", node, err);
                }
            }
        }

        // Reset timer.
        let now = Instant::now();
        let next_tick = now
            .checked_add(self.config.operation_announcement_interval.into())
            .expect("time overflow");
        timer.set(sleep_until(next_tick));
    }

    /// Add an list of operations to a buffer for announcement at the next interval,
    /// or immediately if the buffer is full.
    async fn note_operations_to_announce(
        &mut self,
        operations: &[OperationId],
        timer: &mut Pin<&mut Sleep>,
    ) {
        massa_trace!(
            "protocol.protocol_worker.note_operations_to_announce.begin",
            { "operations": operations }
        );
        // Add the operations to a list for announcement at the next interval.
        self.operations_to_announce.extend_from_slice(operations);

        // If the buffer is full,
        // announce operations immediately,
        // clearing the data at the same time.
        if self.operations_to_announce.len() > self.config.operation_announcement_buffer_capacity {
            self.announce_ops(timer).await;
        }
    }

    async fn propagate_endorsements(&mut self, storage: &Storage) {
        massa_trace!(
            "protocol.protocol_worker.process_command.propagate_endorsements.begin",
            { "endorsements": storage.get_endorsement_refs() }
        );
        for (node, node_info) in self.active_nodes.iter_mut() {
            let new_endorsements: PreHashMap<EndorsementId, SecureShareEndorsement> = {
                let endorsements_reader = storage.read_endorsements();
                storage
                    .get_endorsement_refs()
                    .iter()
                    .filter_map(|id| {
                        if node_info.knows_endorsement(id) {
                            return None;
                        }
                        Some((*id, endorsements_reader.get(id).cloned().unwrap()))
                    })
                    .collect()
            };
            node_info.insert_known_endorsements(new_endorsements.keys().copied());
            let to_send = new_endorsements.into_values().collect::<Vec<_>>();
            if !to_send.is_empty() {
                let res = self
                    .network_command_sender
                    .send_endorsements(*node, to_send)
                    .await;
                if let Err(err) = res {
                    debug!(
                        "could not send endorsements batch to node {}: {}",
                        node, err
                    );
                }
            }
        }
    }

    async fn process_command(
        &mut self,
        cmd: ProtocolCommand,
        block_timer: &mut Pin<&mut Sleep>,
        op_timer: &mut Pin<&mut Sleep>,
    ) -> Result<(), ProtocolError> {
        match cmd {
            ProtocolCommand::IntegratedBlock { block_id, storage } => {
                massa_trace!(
                    "protocol.protocol_worker.process_command.integrated_block.begin",
                    { "block_id": block_id }
                );
                let header = {
                    let blocks = storage.read_blocks();
                    blocks
                        .get(&block_id)
                        .map(|block| block.content.header.clone())
                        .ok_or_else(|| {
                            ProtocolError::ContainerInconsistencyError(format!(
                                "header of id {} not found.",
                                block_id
                            ))
                        })?
                };
                for (node_id, node_info) in self.active_nodes.iter_mut() {
                    // node that isn't asking for that block
                    let cond = node_info.get_known_block(&block_id);
                    // if we don't know if that node knows that hash or if we know it doesn't
                    if !cond.map_or_else(|| false, |v| v.0) {
                        massa_trace!("protocol.protocol_worker.process_command.integrated_block.send_header", { "node": node_id, "block_id": block_id});
                        self.network_command_sender
                            .send_block_header(*node_id, header.clone())
                            .await
                            .map_err(|_| {
                                ProtocolError::ChannelError(
                                    "send block header network command send failed".into(),
                                )
                            })?;
                    } else {
                        massa_trace!("protocol.protocol_worker.process_command.integrated_block.do_not_send", { "node": node_id, "block_id": block_id });
                    }
                }
                massa_trace!(
                    "protocol.protocol_worker.process_command.integrated_block.end",
                    {}
                );
            }
            ProtocolCommand::AttackBlockDetected(block_id) => {
                // Ban all the nodes that sent us this object.
                massa_trace!(
                    "protocol.protocol_worker.process_command.attack_block_detected.begin",
                    { "block_id": block_id }
                );
                let to_ban: Vec<NodeId> = self
                    .active_nodes
                    .iter()
                    .filter_map(|(id, info)| match info.get_known_block(&block_id) {
                        Some((true, _)) => Some(*id),
                        _ => None,
                    })
                    .collect();
                for id in to_ban.iter() {
                    massa_trace!("protocol.protocol_worker.process_command.attack_block_detected.ban_node", { "node": id, "block_id": block_id });
                    self.ban_node(id).await?;
                }
                massa_trace!(
                    "protocol.protocol_worker.process_command.attack_block_detected.end",
                    {}
                );
            }
            ProtocolCommand::WishlistDelta { new, remove } => {
                massa_trace!("protocol.protocol_worker.process_command.wishlist_delta.begin", { "new": new, "remove": remove });
                for (block_id, header) in new.into_iter() {
                    self.block_wishlist.insert(
                        block_id,
                        BlockInfo::new(header, self.storage.clone_without_refs()),
                    );
                }
                // Remove the knowledge that we asked this block to nodes.
                self.remove_asked_blocks_of_node(&remove)?;

                // Remove from the wishlist.
                for block_id in remove.iter() {
                    self.block_wishlist.remove(block_id);
                }
                self.update_ask_block(block_timer).await?;
                massa_trace!(
                    "protocol.protocol_worker.process_command.wishlist_delta.end",
                    {}
                );
            }
            ProtocolCommand::PropagateOperations(storage) => {
                // Note: should we claim refs locally?
                let operation_ids = storage.get_op_refs();
                massa_trace!(
                    "protocol.protocol_worker.process_command.propagate_operations.begin",
                    { "operation_ids": operation_ids }
                );

                // Note operations as checked.
                self.checked_operations
                    .extend(operation_ids.iter().copied());

                // Announce operations to active nodes not knowing about it.
                let to_announce: Vec<OperationId> = operation_ids.iter().copied().collect();
                self.note_operations_to_announce(&to_announce, op_timer)
                    .await;
            }
            ProtocolCommand::PropagateEndorsements(endorsements) => {
                self.propagate_endorsements(&endorsements).await;
            }
        }
        massa_trace!("protocol.protocol_worker.process_command.end", {});
        Ok(())
    }

    /// Remove the given blocks from the local wishlist
    pub(crate) fn remove_asked_blocks_of_node(
        &mut self,
        remove_hashes: &PreHashSet<BlockId>,
    ) -> Result<(), ProtocolError> {
        massa_trace!("protocol.protocol_worker.remove_asked_blocks_of_node", {
            "remove": remove_hashes
        });
        for node_info in self.active_nodes.values_mut() {
            node_info
                .asked_blocks
                .retain(|h, _| !remove_hashes.contains(h));
        }
        Ok(())
    }

    pub(crate) async fn update_ask_block(
        &mut self,
        ask_block_timer: &mut Pin<&mut Sleep>,
    ) -> Result<(), ProtocolError> {
        massa_trace!("protocol.protocol_worker.update_ask_block.begin", {});
        let now = Instant::now();

        // init timer
        let mut next_tick = now
            .checked_add(self.config.ask_block_timeout.into())
            .ok_or(TimeError::TimeOverflowError)?;

        // list blocks to re-ask and gather candidate nodes to ask from
        let mut candidate_nodes: PreHashMap<BlockId, Vec<_>> = Default::default();
        let mut ask_block_list: HashMap<NodeId, Vec<(BlockId, AskForBlocksInfo)>> =
            Default::default();

        // list blocks to re-ask and from whom
        for (hash, block_info) in self.block_wishlist.iter() {
            let required_info = if block_info.header.is_none() {
                AskForBlocksInfo::Header
            } else if block_info.operation_ids.is_none() {
                AskForBlocksInfo::Info
            } else {
                let already_stored_operations = block_info.storage.get_op_refs();
                // Unwrap safety: Check if `operation_ids` is none just above
                AskForBlocksInfo::Operations(
                    block_info
                        .operation_ids
                        .as_ref()
                        .unwrap()
                        .iter()
                        .filter(|id| !already_stored_operations.contains(id))
                        .copied()
                        .collect(),
                )
            };
            let mut needs_ask = true;

            for (node_id, node_info) in self.active_nodes.iter_mut() {
                // map to remove the borrow on asked_blocks. Otherwise can't call insert_known_blocks
                let ask_time_opt = node_info.asked_blocks.get(hash).copied();
                let (timeout_at_opt, timed_out) = if let Some(ask_time) = ask_time_opt {
                    let t = ask_time
                        .checked_add(self.config.ask_block_timeout.into())
                        .ok_or(TimeError::TimeOverflowError)?;
                    (Some(t), t <= now)
                } else {
                    (None, false)
                };
                let knows_block = node_info.get_known_block(hash);

                // check if the node recently told us it doesn't have the block
                if let Some((false, info_time)) = knows_block {
                    let info_expires = info_time
                        .checked_add(self.config.ask_block_timeout.into())
                        .ok_or(TimeError::TimeOverflowError)?;
                    if info_expires > now {
                        next_tick = std::cmp::min(next_tick, info_expires);
                        continue; // ignore candidate node
                    }
                }

                let candidate = match (timed_out, timeout_at_opt, knows_block) {
                    // not asked yet
                    (_, None, knowledge) => match knowledge {
                        Some((true, _)) => (0u8, None),
                        None => (1u8, None),
                        Some((false, _)) => (2u8, None),
                    },
                    // not timed out yet (note: recent DONTHAVBLOCK checked before the match)
                    (false, Some(timeout_at), _) => {
                        next_tick = std::cmp::min(next_tick, timeout_at);
                        needs_ask = false; // no need to re ask
                        continue; // not a candidate
                    }
                    // timed out, supposed to have it
                    (true, Some(timeout_at), Some((true, info_time))) => {
                        if info_time < &timeout_at {
                            // info less recent than timeout: mark as not having it
                            node_info.insert_known_blocks(
                                &[*hash],
                                false,
                                timeout_at,
                                self.config.max_node_known_blocks_size,
                            );
                            (2u8, ask_time_opt)
                        } else {
                            // told us it has it after a timeout: good candidate again
                            (0u8, ask_time_opt)
                        }
                    }
                    // timed out, supposed to not have it
                    (true, Some(timeout_at), Some((false, info_time))) => {
                        if info_time < &timeout_at {
                            // info less recent than timeout: update info time
                            node_info.insert_known_blocks(
                                &[*hash],
                                false,
                                timeout_at,
                                self.config.max_node_known_blocks_size,
                            );
                        }
                        (2u8, ask_time_opt)
                    }
                    // timed out but don't know if has it: mark as not having it
                    (true, Some(timeout_at), None) => {
                        node_info.insert_known_blocks(
                            &[*hash],
                            false,
                            timeout_at,
                            self.config.max_node_known_blocks_size,
                        );
                        (2u8, ask_time_opt)
                    }
                };

                // add candidate node
                candidate_nodes.entry(*hash).or_insert_with(Vec::new).push((
                    candidate,
                    *node_id,
                    required_info.clone(),
                ));
            }

            // remove if doesn't need to be asked
            if !needs_ask {
                candidate_nodes.remove(hash);
            }
        }

        // count active block requests per node
        let mut active_block_req_count: HashMap<NodeId, usize> = self
            .active_nodes
            .iter()
            .map(|(node_id, node_info)| {
                (
                    *node_id,
                    node_info
                        .asked_blocks
                        .iter()
                        .filter(|(_h, ask_t)| {
                            ask_t
                                .checked_add(self.config.ask_block_timeout.into())
                                .map_or(false, |timeout_t| timeout_t > now)
                        })
                        .count(),
                )
            })
            .collect();

        for (hash, criteria) in candidate_nodes.into_iter() {
            // find the best node
            if let Some((_knowledge, best_node, required_info)) = criteria
                .into_iter()
                .filter(|(_knowledge, node_id, _)| {
                    // filter out nodes with too many active block requests
                    *active_block_req_count.get(node_id).unwrap_or(&0)
                        <= self.config.max_simultaneous_ask_blocks_per_node
                })
                .min_by_key(|(knowledge, node_id, _)| {
                    (
                        *knowledge,                                                 // block knowledge
                        *active_block_req_count.get(node_id).unwrap_or(&0), // active requests
                        self.active_nodes.get(node_id).unwrap().connection_instant, // node age (will not panic, already checked)
                        *node_id,                                                   // node ID
                    )
                })
            {
                let info = self.active_nodes.get_mut(&best_node).unwrap(); // will not panic, already checked
                info.asked_blocks.insert(hash, now);
                if let Some(cnt) = active_block_req_count.get_mut(&best_node) {
                    *cnt += 1; // increase the number of actively asked blocks
                }

                ask_block_list
                    .entry(best_node)
                    .or_insert_with(Vec::new)
                    .push((hash, required_info.clone()));

                let timeout_at = now
                    .checked_add(self.config.ask_block_timeout.into())
                    .ok_or(TimeError::TimeOverflowError)?;
                next_tick = std::cmp::min(next_tick, timeout_at);
            }
        }

        // send AskBlockEvents
        if !ask_block_list.is_empty() {
            //massa_trace!("protocol.protocol_worker.update_ask_block", {
            //    "list": ask_block_list
            //});
            self.network_command_sender
                .ask_for_block_list(ask_block_list)
                .await
                .map_err(|_| {
                    ProtocolError::ChannelError("ask for block node command send failed".into())
                })?;
        }

        // reset timer
        ask_block_timer.set(sleep_until(next_tick));

        Ok(())
    }

    /// Ban a node.
    pub(crate) async fn ban_node(&mut self, node_id: &NodeId) -> Result<(), ProtocolError> {
        massa_trace!("protocol.protocol_worker.ban_node", { "node": node_id });
        self.active_nodes.remove(node_id);
        if self.active_nodes.is_empty() {
            info!("Not connected to any peers.");
        }
        self.network_command_sender
            .node_ban_by_ids(vec![*node_id])
            .await
            .map_err(|_| ProtocolError::ChannelError("Ban node command send failed".into()))?;
        Ok(())
    }

    /// Perform checks on a header,
    /// and if valid update the node's view of the world.
    ///
    /// Returns a boolean representing whether the header is new.
    ///
    /// Does not ban the source node if the header is invalid.
    ///
    /// Checks performed on Header:
    /// - Not genesis.
    /// - Can compute a `BlockId`.
    /// - Valid signature.
    /// - Absence of duplicate endorsements.
    ///
    /// Checks performed on endorsements:
    /// - Unique indices.
    /// - Slot matches that of the block.
    /// - Block matches that of the block.
    pub(crate) async fn note_header_from_node(
        &mut self,
        header: &SecuredHeader,
        source_node_id: &NodeId,
    ) -> Result<Option<(BlockId, bool)>, ProtocolError> {
        massa_trace!("protocol.protocol_worker.note_header_from_node", { "node": source_node_id, "header": header });

        // check header integrity
        massa_trace!("protocol.protocol_worker.check_header.start", {
            "header": header
        });

        // refuse genesis blocks
        if header.content.slot.period == self.config.last_start_period
            || header.content.parents.is_empty()
        {
            // genesis
            massa_trace!("protocol.protocol_worker.check_header.err_is_genesis", {
                "header": header
            });
            return Ok(None);
        }

        // compute ID
        let block_id = header.id;

        // check if this header was already verified
        let now = Instant::now();
        if let Some(block_header) = self.checked_headers.get(&block_id) {
            if let Some(node_info) = self.active_nodes.get_mut(source_node_id) {
                node_info.insert_known_blocks(
                    &header.content.parents,
                    true,
                    now,
                    self.config.max_node_known_blocks_size,
                );
                node_info.insert_known_blocks(
                    &[block_id],
                    true,
                    now,
                    self.config.max_node_known_blocks_size,
                );
                node_info.insert_known_endorsements(
                    block_header.content.endorsements.iter().map(|e| e.id),
                );
            }
            return Ok(Some((block_id, false)));
        }

        if let Err(err) = self
            .note_endorsements_from_node(header.content.endorsements.clone(), source_node_id, false)
            .await
        {
            warn!(
                "node {} sent us a header containing critically incorrect endorsements: {}",
                source_node_id, err
            );
            return Ok(None);
        };

        // check header signature
        if let Err(err) = header.verify_signature() {
            massa_trace!("protocol.protocol_worker.check_header.err_signature", { "header": header, "err": format!("{}", err)});
            return Ok(None);
        };

        // check endorsement in header integrity
        let mut used_endorsement_indices: HashSet<u32> =
            HashSet::with_capacity(header.content.endorsements.len());
        for endorsement in header.content.endorsements.iter() {
            // check index reuse
            if !used_endorsement_indices.insert(endorsement.content.index) {
                massa_trace!("protocol.protocol_worker.check_header.err_endorsement_index_reused", { "header": header, "endorsement": endorsement});
                return Ok(None);
            }
            // check slot
            if endorsement.content.slot != header.content.slot {
                massa_trace!("protocol.protocol_worker.check_header.err_endorsement_invalid_slot", { "header": header, "endorsement": endorsement});
                return Ok(None);
            }
            // check endorsed block
            if endorsement.content.endorsed_block
                != header.content.parents[header.content.slot.thread as usize]
            {
                massa_trace!("protocol.protocol_worker.check_header.err_endorsement_invalid_endorsed_block", { "header": header, "endorsement": endorsement});
                return Ok(None);
            }
        }

        self.checked_headers.insert(block_id, header.clone());

        if let Some(node_info) = self.active_nodes.get_mut(source_node_id) {
            node_info.insert_known_blocks(
                &header.content.parents,
                true,
                now,
                self.config.max_node_known_blocks_size,
            );
            node_info.insert_known_blocks(
                &[block_id],
                true,
                now,
                self.config.max_node_known_blocks_size,
            );
            node_info.insert_known_endorsements(header.content.endorsements.iter().map(|e| e.id));
            massa_trace!("protocol.protocol_worker.note_header_from_node.ok", { "node": source_node_id,"block_id":block_id, "header": header});
            return Ok(Some((block_id, true)));
        }
        Ok(None)
    }

    /// Checks operations, caching knowledge of valid ones.
    ///
    /// Does not ban if the operation is invalid.
    ///
    /// Checks performed:
    /// - Valid signature
    pub(crate) async fn note_operations_from_node(
        &mut self,
        operations: Vec<SecureShareOperation>,
        source_node_id: &NodeId,
        op_timer: &mut Pin<&mut Sleep>,
    ) -> Result<(), ProtocolError> {
        massa_trace!("protocol.protocol_worker.note_operations_from_node", { "node": source_node_id, "operations": operations });
        let length = operations.len();
        let mut new_operations = PreHashMap::with_capacity(length);
        let mut received_ids = PreHashSet::with_capacity(length);
        for operation in operations {
            let operation_id = operation.id;
            if operation.serialized_size() > self.config.max_serialized_operations_size_per_block {
                return Err(ProtocolError::InvalidOperationError(format!(
                    "Operation {} exceeds max block size,  maximum authorized {} bytes but found {} bytes",
                    operation_id,
                    operation.serialized_size(),
                    self.config.max_serialized_operations_size_per_block
                )));
            };
            received_ids.insert(operation_id);

            // Check operation signature only if not already checked.
            if !self.checked_operations.contains_id(&operation_id) {
                // check signature if the operation wasn't in `checked_operation`
                new_operations.insert(operation_id, operation);
            };
        }

        // optimized signature verification
        verify_sigs_batch(
            &new_operations
                .iter()
                .map(|(op_id, op)| (*op_id.get_hash(), op.signature, op.content_creator_pub_key))
                .collect::<Vec<_>>(),
        )?;

        // add to checked operations
        self.checked_operations
            .extend(new_operations.keys().copied());

        // add to known ops
        if let Some(node_info) = self.active_nodes.get_mut(source_node_id) {
            node_info.insert_known_ops(received_ids.iter().map(|id| id.prefix()));
        }

        if !new_operations.is_empty() {
            // Store operation, claim locally
            let mut ops = self.storage.clone_without_refs();
            ops.store_operations(new_operations.into_values().collect());

            // Propagate operations when their expire period isn't `max_operations_propagation_time` old.
            let mut ops_to_propagate = ops.clone();
            let operations_to_not_propagate = {
                let now = MassaTime::now()?;
                let read_operations = ops_to_propagate.read_operations();
                ops_to_propagate
                    .get_op_refs()
                    .iter()
                    .filter(|op_id| {
                        let expire_period =
                            read_operations.get(op_id).unwrap().content.expire_period;
                        let expire_period_timestamp = get_block_slot_timestamp(
                            self.config.thread_count,
                            self.config.t0,
                            self.config.genesis_timestamp,
                            Slot::new(expire_period, 0),
                        );
                        match expire_period_timestamp {
                            Ok(slot_timestamp) => {
                                slot_timestamp
                                    .saturating_add(self.config.max_operations_propagation_time)
                                    < now
                            }
                            Err(_) => true,
                        }
                    })
                    .copied()
                    .collect()
            };
            ops_to_propagate.drop_operation_refs(&operations_to_not_propagate);
            let to_announce: Vec<OperationId> =
                ops_to_propagate.get_op_refs().iter().copied().collect();
            self.note_operations_to_announce(&to_announce, op_timer)
                .await;

            // Add to pool
            self.pool_controller.add_operations(ops);
        }

        Ok(())
    }

    /// Note endorsements coming from a given node,
    /// and propagate them when they were received outside of a header.
    ///
    /// Caches knowledge of valid ones.
    ///
    /// Does not ban if the endorsement is invalid
    ///
    /// Checks performed:
    /// - Valid signature.
    pub(crate) async fn note_endorsements_from_node(
        &mut self,
        endorsements: Vec<SecureShareEndorsement>,
        source_node_id: &NodeId,
        propagate: bool,
    ) -> Result<(), ProtocolError> {
        massa_trace!("protocol.protocol_worker.note_endorsements_from_node", { "node": source_node_id, "endorsements": endorsements});
        let length = endorsements.len();
        let mut new_endorsements = PreHashMap::with_capacity(length);
        let mut endorsement_ids = PreHashSet::with_capacity(length);
        for endorsement in endorsements.into_iter() {
            let endorsement_id = endorsement.id;
            endorsement_ids.insert(endorsement_id);

            // check endorsement signature if not already checked
            if !self.checked_endorsements.contains(&endorsement_id) {
                new_endorsements.insert(endorsement_id, endorsement);
            }
        }

        // Batch signature verification
        // optimized signature verification
        verify_sigs_batch(
            &new_endorsements
                .values()
                .map(|endorsement| {
                    (
                        endorsement.compute_signed_hash(),
                        endorsement.signature,
                        endorsement.content_creator_pub_key,
                    )
                })
                .collect::<Vec<_>>(),
        )?;

        // add to verified signature cache
        self.checked_endorsements
            .try_extend(endorsement_ids.iter().copied());

        // add to known endorsements for source node.
        if let Some(node_info) = self.active_nodes.get_mut(source_node_id) {
            node_info.insert_known_endorsements(endorsement_ids);
        }

        if !new_endorsements.is_empty() {
            let mut endorsements = self.storage.clone_without_refs();
            endorsements.store_endorsements(new_endorsements.into_values().collect());

            // Propagate endorsements
            if propagate {
                // Propagate endorsements when the slot of the block they endorse isn't `max_endorsements_propagation_time` old.
                let mut endorsements_to_propagate = endorsements.clone();
                let endorsements_to_not_propagate = {
                    let now = MassaTime::now()?;
                    let read_endorsements = endorsements_to_propagate.read_endorsements();
                    endorsements_to_propagate
                        .get_endorsement_refs()
                        .iter()
                        .filter_map(|endorsement_id| {
                            let slot_endorsed_block =
                                read_endorsements.get(endorsement_id).unwrap().content.slot;
                            let slot_timestamp = get_block_slot_timestamp(
                                self.config.thread_count,
                                self.config.t0,
                                self.config.genesis_timestamp,
                                slot_endorsed_block,
                            );
                            match slot_timestamp {
                                Ok(slot_timestamp) => {
                                    if slot_timestamp.saturating_add(
                                        self.config.max_endorsements_propagation_time,
                                    ) < now
                                    {
                                        Some(*endorsement_id)
                                    } else {
                                        None
                                    }
                                }
                                Err(_) => Some(*endorsement_id),
                            }
                        })
                        .collect()
                };
                endorsements_to_propagate.drop_endorsement_refs(&endorsements_to_not_propagate);
                self.propagate_endorsements(&endorsements_to_propagate)
                    .await;
            }

            // Add to pool
            self.pool_controller.add_endorsements(endorsements);
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::node_info::NodeInfo;
    use massa_hash::Hash;
    use massa_protocol_exports::{tests::tools::create_protocol_config, ProtocolConfig};
    use serial_test::serial;

    lazy_static::lazy_static! {
        static ref PROTOCOL_CONFIG: ProtocolConfig = create_protocol_config();
    }

    pub fn get_dummy_block_id(s: &str) -> BlockId {
        BlockId(Hash::compute_from(s.as_bytes()))
    }

    #[test]
    #[serial]
    fn test_node_info_know_block() {
        let max_node_known_blocks_size = 10;
        let config = &PROTOCOL_CONFIG;
        let mut nodeinfo = NodeInfo::new(config);
        let instant = Instant::now();

        let hash_test = get_dummy_block_id("test");
        nodeinfo.insert_known_blocks(&[hash_test], true, instant, max_node_known_blocks_size);
        let (val, t) = nodeinfo.get_known_block(&hash_test).unwrap();
        assert!(val);
        assert_eq!(instant, *t);
        nodeinfo.insert_known_blocks(&[hash_test], false, instant, max_node_known_blocks_size);
        let (val, t) = nodeinfo.get_known_block(&hash_test).unwrap();
        assert!(!val);
        assert_eq!(instant, *t);

        for index in 0..9 {
            let hash = get_dummy_block_id(&index.to_string());
            nodeinfo.insert_known_blocks(&[hash], true, Instant::now(), max_node_known_blocks_size);
            assert!(nodeinfo.get_known_block(&hash).is_some());
        }

        // re insert the oldest to update its timestamp.
        nodeinfo.insert_known_blocks(
            &[hash_test],
            false,
            Instant::now(),
            max_node_known_blocks_size,
        );

        // add hash that triggers container pruning
        nodeinfo.insert_known_blocks(
            &[get_dummy_block_id("test2")],
            true,
            Instant::now(),
            max_node_known_blocks_size,
        );

        // test should be present
        assert!(nodeinfo
            .get_known_block(&get_dummy_block_id("test"))
            .is_some());
        // 0 should be remove because it's the oldest.
        assert!(nodeinfo
            .get_known_block(&get_dummy_block_id(&0.to_string()))
            .is_none());
        // the other are still present.
        for index in 1..9 {
            let hash = get_dummy_block_id(&index.to_string());
            assert!(nodeinfo.get_known_block(&hash).is_some());
        }
    }
}
