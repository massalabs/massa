//! Copyright (c) 2022 MASSA LABS <info@massa.net>

use crate::{node_info::NodeInfo, worker_operations_impl::OperationBatchBuffer};

use massa_logging::massa_trace;

use massa_models::block::Block;
use massa_models::cache::{HashCacheMap, HashCacheSet};
use massa_models::endorsement::Endorsement;
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
use std::collections::{hash_map, HashMap, HashSet};
use tokio::{
    sync::mpsc,
    sync::mpsc::error::SendTimeoutError,
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
/// * `network_command_sender`: the `NetworkCommandSender` we interact with
/// * `network_event_receiver`: the `NetworkEventReceiver` we interact with
/// * `storage`: Shared storage to fetch data that are fetch across all modules
pub async fn start_protocol_controller(
    config: ProtocolConfig,
    network_command_sender: NetworkCommandSender,
    network_event_receiver: NetworkEventReceiver,
    pool_controller: Box<dyn PoolController>,
    storage: Storage,
) -> Result<
    (
        ProtocolCommandSender,
        ProtocolEventReceiver,
        ProtocolManager,
    ),
    ProtocolError,
> {
    debug!("starting protocol controller");

    // launch worker
    let (controller_event_tx, event_rx) = mpsc::channel::<ProtocolEvent>(config.event_channel_size);
    let (command_tx, controller_command_rx) =
        mpsc::channel::<ProtocolCommand>(config.controller_channel_size);
    let (manager_tx, controller_manager_rx) = mpsc::channel::<ProtocolManagementCommand>(1);
    let pool_controller = pool_controller.clone();
    let join_handle = tokio::spawn(async move {
        let res = ProtocolWorker::new(
            config,
            ProtocolWorkerChannels {
                network_command_sender,
                network_event_receiver,
                controller_event_tx,
                controller_command_rx,
                controller_manager_rx,
            },
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
    Ok((
        ProtocolCommandSender(command_tx),
        ProtocolEventReceiver(event_rx),
        ProtocolManager::new(join_handle, manager_tx),
    ))
}

/// An enum determining the status of block retrieval
enum BlockRetrievalStatus {
    /// Operation list being queried
    QueryingOpList {
        /// block header
        header: WrappedHeader,
        /// node being queried
        queried_node: NodeId,
        /// timestamp of the query
        timestamp: Instant,
    },
    /// Operation list being queried
    QueryingOperations {
        /// block header
        header: WrappedHeader,
        /// ordered operation list
        operation_ids: Vec<OperationId>,
        /// storage with referencing endorsements and already-available operations
        block_storage: Storage,
        /// node being queried
        queried_node: NodeId,
        /// timestamp of the query
        timestamp: Instant,
    },
}

/// protocol worker
pub struct ProtocolWorker {
    /// Protocol configuration.
    config: ProtocolConfig,
    /// Associated network command sender.
    network_command_sender: NetworkCommandSender,
    /// Associated network event receiver.
    network_event_receiver: NetworkEventReceiver,
    /// Channel to send protocol events to the controller.
    controller_event_tx: mpsc::Sender<ProtocolEvent>,
    /// Channel to send protocol pool events to the controller.
    pool_controller: Box<dyn PoolController>,
    /// Channel receiving commands from the controller.
    controller_command_rx: mpsc::Receiver<ProtocolCommand>,
    /// Channel to send management commands to the controller.
    controller_manager_rx: mpsc::Receiver<ProtocolManagementCommand>,
    /// Shared storage.
    storage: Storage,

    /// Ids of active nodes mapped to node info.
    active_nodes: HashMap<NodeId, NodeInfo>,

    /// Cache of retrieved endorsements
    retrieved_endorsements_cache: HashCacheSet<BlockId>,

    /// List of blocks required for retrieval (must keep retrying)
    block_wishlist: PreHashMap<BlockId, Option<WrappedHeader>>,
    /// List of wanted blocks and their retrieval status
    block_retrieval: PreHashMap<BlockId, BlockRetrievalStatus>,
    /// Cache of retrieved blocks (mapped to the list od endorsements/operations in the block)
    retrieved_blocks_cache:
        HashCacheMap<BlockId, (PreHashSet<EndorsementId>, PreHashSet<OperationId>)>,

    /// Cache of retrieved operations
    retrieved_ops_cache: HashCacheSet<OperationId>,
    /// Cache map of operation prefixes
    operation_prefix_cache: HashCacheMap<OperationPrefixId, OperationId>,
    /// List of ids of operations that we asked to the nodes
    asked_operations: HashMap<OperationPrefixId, (Instant, Vec<NodeId>)>,
    /// Buffer for operations that we want later
    op_batch_buffer: OperationBatchBuffer,
}

/// channels used by the protocol worker
pub struct ProtocolWorkerChannels {
    /// network command sender
    pub network_command_sender: NetworkCommandSender,
    /// network event receiver
    pub network_event_receiver: NetworkEventReceiver,
    /// protocol event sender
    pub controller_event_tx: mpsc::Sender<ProtocolEvent>,
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
            controller_event_tx,
            controller_command_rx,
            controller_manager_rx,
        }: ProtocolWorkerChannels,
        pool_controller: Box<dyn PoolController>,
        storage: Storage,
    ) -> ProtocolWorker {
        ProtocolWorker {
            config,
            network_command_sender,
            network_event_receiver,
            controller_event_tx,
            pool_controller,
            controller_command_rx,
            controller_manager_rx,
            active_nodes: Default::default(),
            retrieved_endorsements_cache: HashCacheSet::new(config.max_known_endorsements_size),
            operation_prefix_cache: HashCacheMap::new(config.max_known_ops_size),
            block_wishlist: Default::default(),
            block_retrieval: Default::default(),
            retrieved_blocks_cache: HashCacheMap::new(config.max_known_blocks_size),
            retrieved_ops_cache: HashCacheSet::new(config.max_known_ops_size),
            asked_operations: Default::default(),
            op_batch_buffer: OperationBatchBuffer::with_capacity(
                config.operation_batch_buffer_capacity,
            ),
            storage,
        }
    }

    /// Emit an event
    pub(crate) async fn send_protocol_event(&self, event: ProtocolEvent) {
        let result = self
            .controller_event_tx
            .send_timeout(event, self.config.max_send_wait.to_duration())
            .await;
        match result {
            Ok(()) => {}
            Err(SendTimeoutError::Closed(event)) => {
                warn!(
                    "Failed to send ProtocolEvent due to channel closure: {:?}.",
                    event
                );
            }
            Err(SendTimeoutError::Timeout(event)) => {
                warn!("Failed to send ProtocolEvent due to timeout: {:?}.", event);
            }
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
        let operation_prune_timer = sleep(self.config.asked_operations_pruning_period.into());
        tokio::pin!(operation_prune_timer);
        let block_ask_timer = sleep(self.config.ask_block_timeout.into());
        tokio::pin!(block_ask_timer);
        let operation_batch_proc_period_timer =
            sleep(self.config.operation_batch_proc_period.into());
        tokio::pin!(operation_batch_proc_period_timer);
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
                    self.process_command(cmd, &mut block_ask_timer).await?;
                }

                // listen to network controller events
                evt = self.network_event_receiver.wait_event() => {
                    massa_trace!("protocol.protocol_worker.run_loop.network_event_rx", {});
                    self.on_network_event(evt?, &mut block_ask_timer).await?;
                }

                // block ask timer
                _ = &mut block_ask_timer => {
                    massa_trace!("protocol.protocol_worker.run_loop.block_ask_timer", { });
                    self.update_ask_block(&mut block_ask_timer).await?;
                }

                // operation ask timer
                _ = &mut operation_batch_proc_period_timer => {
                    massa_trace!("protocol.protocol_worker.run_loop.operation_ask_timer", { });
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

    /// propagate a block
    async fn propagate_block(
        &mut self,
        block_id: &BlockId,
        storage: Storage,
    ) -> Result<(), ProtocolError> {
        massa_trace!(
            "protocol.protocol_worker.process_command.integrated_block.begin",
            { "block_id": block_id }
        );

        // mark that we know the block
        self.retrieved_blocks_cache.insert(*block_id);

        // retrieve header
        let header = {
            let blocks = self.storage.read_blocks();
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

        // Note that we drop the storage here for simplicity for now, and just not send things we don't have.
        // In the future we could keep it during the whole propagation process to ensure we have everything in store.

        for (node_id, node_info) in self.active_nodes.iter_mut() {
            if node_info.known_blocks.contains(block_id) {
                // this node already knows about that block
                continue;
            }
            massa_trace!("protocol.protocol_worker.process_command.integrated_block.send_header", { "node": node_id, "block_id": block_id});
            // announce header to that node
            self.network_command_sender
                .send_block_header(*node_id, header.clone())
                .await
                .map_err(|_| {
                    ProtocolError::ChannelError(
                        "send block header network command send failed".into(),
                    )
                })?;
        }
        massa_trace!(
            "protocol.protocol_worker.process_command.integrated_block.end",
            {}
        );

        Ok(())
    }

    /// ban all nodes that have an attack block
    async fn ban_nodes_having_block(&mut self, block_id: &BlockId) -> Result<(), ProtocolError> {
        massa_trace!(
            "protocol.protocol_worker.process_command.attack_block_detected.begin",
            { "block_id": block_id }
        );
        let to_ban: Vec<NodeId> = self
            .active_nodes
            .iter()
            .filter(|(_, info)| info.known_blocks.contains(block_id))
            .map(|(id, _)| *id)
            .collect();
        for id in to_ban {
            massa_trace!("protocol.protocol_worker.process_command.attack_block_detected.ban_node", { "node": id, "block_id": block_id });
            self.ban_node(&id).await?;
        }
        massa_trace!(
            "protocol.protocol_worker.process_command.attack_block_detected.end",
            {}
        );
        Ok(())
    }

    /// Propagate operations to nodes that don't already have them
    async fn propagate_ops(&mut self, storage: Storage) -> Resutl<(), ProtocolError> {
        let operation_ids = storage.get_op_refs();
        massa_trace!(
            "protocol.protocol_worker.process_command.propagate_operations.begin",
            { "operation_ids": operation_ids }
        );

        // mark us as having those ops
        operation_ids
            .iter()
            .for_each(|id| self.retrieved_ops_cache.insert(*id));

        // For now we drop storage here for simplicity, and simply don't propagate missing ops.
        // In the future we might keep it until propagation finishes.

        for (node_id, node_info) in self.active_nodes.iter_mut() {
            // add to node knowledge, list all those the node didn't have
            let prefixes_to_announce: Vec<_> = operation_ids
                .iter()
                .filter(|id| node_info.known_operations.insert(*id))
                .map(|id| id.into_prefix())
                .collect();
            if !prefixes_to_announce.is_empty() {
                self.network_command_sender
                    .send_operations_batch(*node_id, prefixes_to_announce)
                    .await?;
            }
        }

        Ok(())
    }

    /// Propagate endorsments to nodes that don't have it yet
    async fn propagate_endorsements(&mut self, storage: Storage) -> Result<(), ProtocolError> {
        let endorsement_ids = storage.get_endorsement_refs();
        massa_trace!(
            "protocol.protocol_worker.process_command.propagate_endorsements.begin",
            { "endorsement_ids": endorsement_ids }
        );

        // mark us as having those endorsements
        endorsement_ids
            .iter()
            .for_each(|id| self.retrieved_endorsements_cache.insert(*id));

        // For now we drop storage here for simplicity, and simply don't propagate missing endorsements.
        // In the future we might keep it until propagation finishes.

        for (node_id, node_info) in self.active_nodes.iter_mut() {
            // add to node knowledge, list all those the node didn't have
            let endorsements_to_propagate: Vec<_> = {
                let store = storage.read_endorsements();
                endorsement_ids
                    .iter()
                    .filter(|id| node_info.known_endorsements.insert(*id))
                    .filter_map(|id| store.get(id))
                    .cloned()
                    .collect()
            };
            if !endorsements_to_propagate.is_empty() {
                self.network_command_sender
                    .send_endorsements(*node_id, endorsements_to_propagate)
                    .await?;
            }
        }

        Ok(())
    }

    /// updates block wishlist
    async fn update_block_wishlist(
        &mut self,
        add: PreHashMap<BlockId, Option<WrappedHeader>>,
        remove: PreHashSet<BlockId>,
    ) -> Result<(), ProtocolError> {
        massa_trace!(
            "protocol.protocol_worker.process_command.retreive_blocks.begin",
            { "add": add, "remove": remove }
        );

        // add new entires
        for (id, opt_header) in add {
            let cur_entry = self.block_wishlist.entry(key).or_insert(opt_header);
            if cur_entry.is_none() && opt_header.is_some() {
                *cur_entry = opt_header;
            }
        }

        // remove entries that are not needed anymore
        remove
            .into_iter()
            .for_each(|id| self.block_wishlist.remove(&id));

        // update ask block process
        self.update_ask_block(timer).await?;
        massa_trace!(
            "protocol.protocol_worker.process_command.retreive_blocks.end",
            {}
        );

        Ok(())
    }

    /// process incoming command
    async fn process_command(
        &mut self,
        cmd: ProtocolCommand,
        timer: &mut std::pin::Pin<&mut Sleep>,
    ) -> Result<(), ProtocolError> {
        match cmd {
            ProtocolCommand::PropagateBlock { block_id, storage } => {
                // Propagate block to all nodes that don't have it
                self.propagate_block(&block_id, storage).await?;
            }
            ProtocolCommand::AttackBlockDetected(block_id) => {
                // Ban all the nodes that have a block.
                self.ban_nodes_having_block(&block_id).await?;
            }
            ProtocolCommand::BlockWishListDelta { add, remove } => {
                // update block wishlist
                self.update_block_wishlist(add, remove).await?;
            }
            ProtocolCommand::PropagateOperations(storage) => {
                // propagate ops to nodes that don't have them yet
                self.propagate_ops(storage).await?;
            }
            ProtocolCommand::PropagateEndorsements(endorsements) => {
                // propagate endorsements to nodes that don't have them yet
                self.propagate_endorsements(storage).await?;
            }
        }
        massa_trace!("protocol.protocol_worker.process_command.end", {});
        Ok(())
    }

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

        // check process status and accumulate node load (numbe rof blocks being asked to that node)
        let mut node_load: HashMap<NodeId, usize> =
            self.active_nodes.keys().map(|id| (*id, 0)).collect();
        let block_ids: Vec<_> = self.block_retrieval.keys().copied().collect();
        for block_id in block_ids {
            let mut entry = match self.block_retrieval.entry(block_id) {
                hash_map::Entry::Vacant(_) => continue,
                hash_map::Entry::Occupied(mut occ) => occ
            };
            match entry.get() {
                /// Operation list being queried
                QueryingOpList {
                    queried_node,
                    timestamp,
                    ..
                } => {
                    // check for timeout or node absence
                    if now.saturating_duration_since(timestamp) > self.config.ask_block_timeout
                        || !self.active_nodes.contains_key(queried_node)
                    {
                        if let Some(node_info) = self.active_nodes.get_mut(queried_node) {
                            // mark the node as not having the block
                            node_info.known_blocks.remove(&block_id);
                        }
                        
                    } else {
                        // update node load
                        let load = node_load.get_mut(queried_node).unwrap();
                        *load = load.saturating_add(1);
                    }
                }
                /// Operations being queried
                BlockRetrievalStatus::QueryingOperations {
                    header,
                    operation_ids,
                    block_storage,
                    queried_node,
                    timestamp,
                } => {
                    // TODO if timeout or node absent mark node as not knowing it, remove from status
                }
            }
        }

        TODO

        // reset timer
        ask_block_timer.set(sleep_until(next_tick));

        Ok(())
    }

    /// Ban a node.
    pub(crate) async fn ban_node(&mut self, node_id: &NodeId) -> Result<(), ProtocolError> {
        massa_trace!("protocol.protocol_worker.ban_node", { "node": node_id });
        self.active_nodes.remove(node_id);
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
        header: &WrappedHeader,
        source_node_id: &NodeId,
    ) -> Result<Option<(BlockId, PreHashMap<EndorsementId, u32>, bool)>, ProtocolError> {
        massa_trace!("protocol.protocol_worker.note_header_from_node", { "node": source_node_id, "header": header });

        // check header integrity
        massa_trace!("protocol.protocol_worker.check_header.start", {
            "header": header
        });

        // refuse genesis blocks
        if header.content.slot.period == 0 || header.content.parents.is_empty() {
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
        if let Some(block_info) = self.checked_headers.get(&block_id) {
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
                    block_info.endorsements.keys().copied().collect(),
                    self.config.max_node_known_endorsements_size,
                );
                if let Some(operations) = block_info.operations.as_ref() {
                    node_info.insert_known_ops(
                        operations.iter().cloned().collect(),
                        self.config.max_node_known_ops_size,
                    );
                }
            }
            return Ok(Some((block_id, block_info.endorsements.clone(), false)));
        }

        let (endorsement_ids, endorsements_reused) = match self.note_endorsements_from_node(
            header.content.endorsements.clone(),
            source_node_id,
            false,
        ) {
            Err(_) => {
                warn!(
                    "node {} sent us a header containing critically incorrect endorsements",
                    source_node_id
                );
                return Ok(None);
            }
            Ok(id) => id,
        };

        // check if some endorsements are duplicated in the header
        if endorsements_reused {
            massa_trace!(
                "protocol.protocol_worker.check_header.err_endorsement_reused",
                { "header": header }
            );
            return Ok(None);
        }

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
            if (endorsement.content.slot.thread != header.content.slot.thread)
                || (endorsement.content.slot >= header.content.slot)
            {
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

        let block_info = BlockInfo::new(endorsement_ids.clone(), header.clone());
        if self
            .checked_headers
            .insert(block_id, block_info.clone())
            .is_none()
        {
            self.prune_checked_headers();
        }

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
                block_info.endorsements.keys().copied().collect(),
                self.config.max_node_known_endorsements_size,
            );
            if let Some(operations) = block_info.operations.as_ref() {
                node_info.insert_known_ops(
                    operations.iter().cloned().collect(),
                    self.config.max_node_known_ops_size,
                );
            }
            massa_trace!("protocol.protocol_worker.note_header_from_node.ok", { "node": source_node_id,"block_id":block_id, "header": header});
            return Ok(Some((block_id, endorsement_ids, true)));
        }
        Ok(None)
    }

    /// Checks operations, caching knowledge of valid ones.
    ///
    /// Does not ban if the operation is invalid.
    ///
    /// Returns :
    /// - a list of seen operation ids, for use in checking the root hash of the block.
    /// - a map of seen operations with indices and validity periods to avoid recomputing them later
    /// - the sum of all operation's `max_gas`.
    ///
    /// Checks performed:
    /// - Valid signature
    pub(crate) fn note_operations_from_node(
        &mut self,
        operations: Vec<WrappedOperation>,
        source_node_id: &NodeId,
    ) -> Result<(Vec<OperationId>, PreHashMap<OperationId, usize>), ProtocolError> {
        massa_trace!("protocol.protocol_worker.note_operations_from_node", { "node": source_node_id, "operations": operations });
        let length = operations.len();
        let mut seen_ops = vec![];
        let mut new_operations = PreHashMap::with_capacity(length);
        let mut received_ids = PreHashMap::with_capacity(length);
        for (idx, operation) in operations.into_iter().enumerate() {
            let operation_id = operation.id;
            seen_ops.push(operation_id);
            received_ids.insert(operation_id, idx);
            // Check operation signature only if not already checked.
            if self.checked_operations.insert(&operation_id) {
                // check signature if the operation wasn't in `checked_operation`
                operation.verify_signature()?;
                new_operations.insert(operation_id, operation);
            };
        }

        // add to known ops
        if let Some(node_info) = self.active_nodes.get_mut(source_node_id) {
            node_info.insert_known_ops(
                received_ids.keys().copied().collect(),
                self.config.max_node_known_ops_size,
            );
        }

        if !new_operations.is_empty() {
            // prune checked operations cache
            self.prune_checked_operations();

            // Store operation, claim locally
            let mut ops = self.storage.clone_without_refs();
            ops.store_operations(new_operations.into_values().collect());

            // Add to pool, propagate when received outside of a header.
            self.pool_controller.add_operations(ops);
        }

        Ok((seen_ops, received_ids))
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
    pub(crate) fn note_endorsements_from_node(
        &mut self,
        endorsements: Vec<WrappedEndorsement>,
        source_node_id: &NodeId,
        propagate: bool,
    ) -> Result<(PreHashMap<EndorsementId, u32>, bool), ProtocolError> {
        massa_trace!("protocol.protocol_worker.note_endorsements_from_node", { "node": source_node_id, "endorsements": endorsements});
        let length = endorsements.len();
        let mut contains_duplicates = false;

        let mut new_endorsements = PreHashMap::with_capacity(length);
        let mut endorsement_ids = PreHashMap::default();
        for endorsement in endorsements.into_iter() {
            let endorsement_id = endorsement.id;
            if endorsement_ids
                .insert(endorsement_id, endorsement.content.index)
                .is_some()
            {
                contains_duplicates = true;
            }
            // check endorsement signature if not already checked
            if self.checked_endorsements.insert(endorsement_id) {
                endorsement.verify_signature()?;
                new_endorsements.insert(endorsement_id, endorsement);
            }
        }

        // add to known endorsements for source node.
        if let Some(node_info) = self.active_nodes.get_mut(source_node_id) {
            node_info.insert_known_endorsements(
                endorsement_ids.keys().copied().collect(),
                self.config.max_node_known_endorsements_size,
            );
        }

        if !new_endorsements.is_empty() && propagate {
            self.prune_checked_endorsements();

            let mut endorsements = self.storage.clone_without_refs();
            endorsements.store_endorsements(new_endorsements.into_values().collect());

            // Add to pool, propagate if required
            self.pool_controller.add_endorsements(endorsements);
        }

        Ok((endorsement_ids, contains_duplicates))
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
