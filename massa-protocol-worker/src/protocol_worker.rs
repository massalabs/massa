// Copyright (c) 2022 MASSA LABS <info@massa.net>

use itertools::Itertools;
use massa_hash::hash::Hash;
use massa_logging::massa_trace;
use massa_models::{
    constants::CHANNEL_SIZE,
    node::NodeId,
    prehash::{BuildMap, Map, Set},
    Address, Block, BlockHeader, BlockId, Endorsement, EndorsementId, Operation, OperationId,
    OperationType,
};
use massa_network::{NetworkCommandSender, NetworkEvent, NetworkEventReceiver};
use massa_protocol_exports::{
    ProtocolCommand, ProtocolCommandSender, ProtocolError, ProtocolEvent, ProtocolEventReceiver,
    ProtocolManagementCommand, ProtocolManager, ProtocolPoolEvent, ProtocolPoolEventReceiver,
    ProtocolSettings,
};
use massa_time::TimeError;
use std::collections::{HashMap, HashSet};
use tokio::{
    sync::mpsc,
    sync::mpsc::error::SendTimeoutError,
    time::{sleep, sleep_until, Instant, Sleep},
};
use tracing::{debug, error, info, warn};

/// start a new ProtocolController from a ProtocolConfig
/// - generate public / private key
/// - create protocol_command/protocol_event channels
/// - launch protocol_controller_fn in an other task
///
/// # Arguments
/// * cfg: protocol configuration
/// * operation_validity_periods: operation validity duration in periods
/// * max_block_gas: maximum gas per block
/// * network_command_sender: the NetworkCommandSender we interact with
/// * network_event_receiver: the NetworkEventReceiver we interact with
pub async fn start_protocol_controller(
    protocol_settings: &'static ProtocolSettings,
    operation_validity_periods: u64,
    max_block_gas: u64,
    network_command_sender: NetworkCommandSender,
    network_event_receiver: NetworkEventReceiver,
) -> Result<
    (
        ProtocolCommandSender,
        ProtocolEventReceiver,
        ProtocolPoolEventReceiver,
        ProtocolManager,
    ),
    ProtocolError,
> {
    debug!("starting protocol controller");

    // launch worker
    let (controller_event_tx, event_rx) = mpsc::channel::<ProtocolEvent>(CHANNEL_SIZE);
    let (controller_pool_event_tx, pool_event_rx) =
        mpsc::channel::<ProtocolPoolEvent>(CHANNEL_SIZE);
    let (command_tx, controller_command_rx) = mpsc::channel::<ProtocolCommand>(CHANNEL_SIZE);
    let (manager_tx, controller_manager_rx) = mpsc::channel::<ProtocolManagementCommand>(1);
    let join_handle = tokio::spawn(async move {
        let res = ProtocolWorker::new(
            protocol_settings,
            operation_validity_periods,
            max_block_gas,
            ProtocolWorkerChannels {
                network_command_sender,
                network_event_receiver,
                controller_event_tx,
                controller_pool_event_tx,
                controller_command_rx,
                controller_manager_rx,
            },
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
        ProtocolPoolEventReceiver(pool_event_rx),
        ProtocolManager::new(join_handle, manager_tx),
    ))
}

//put in a module to block private access from Protocol_worker.
mod nodeinfo {
    use massa_models::prehash::{BuildMap, Map, Set};
    use massa_models::{BlockId, EndorsementId, OperationId};
    use massa_protocol_exports::ProtocolSettings;
    use std::collections::VecDeque;
    use tokio::time::Instant;

    /// Information about a node we are connected to,
    /// essentially our view of its state.
    ///
    /// Note: should we prune the set of known and wanted blocks during lifetime of a node connection?
    /// Currently it would only be dropped alongside the rest when the node becomes inactive.
    #[derive(Debug, Clone)]
    pub struct NodeInfo {
        /// The blocks the node "knows about",
        /// defined as the one the node propagated headers to us for.
        pub known_blocks: Map<BlockId, (bool, Instant)>,
        /// The blocks the node asked for.
        pub wanted_blocks: Map<BlockId, Instant>,
        /// Blocks we asked that node for
        pub asked_blocks: Map<BlockId, Instant>,
        /// Instant when the node was added
        pub connection_instant: Instant,
        /// all known operations
        pub known_operations: Set<OperationId>,
        pub known_operations_queue: VecDeque<OperationId>,
        /// all known endorsements
        pub known_endorsements: Set<EndorsementId>,
        pub known_endorsements_queue: VecDeque<EndorsementId>,
    }

    impl NodeInfo {
        /// Creates empty node info
        pub fn new(pool_settings: &'static ProtocolSettings) -> NodeInfo {
            NodeInfo {
                known_blocks: Map::with_capacity_and_hasher(
                    pool_settings.max_node_known_blocks_size,
                    BuildMap::default(),
                ),
                wanted_blocks: Map::with_capacity_and_hasher(
                    pool_settings.max_node_wanted_blocks_size,
                    BuildMap::default(),
                ),
                asked_blocks: Default::default(),
                connection_instant: Instant::now(),
                known_operations: Set::<OperationId>::with_capacity_and_hasher(
                    pool_settings.max_known_ops_size,
                    BuildMap::default(),
                ),
                known_operations_queue: VecDeque::with_capacity(pool_settings.max_known_ops_size),
                known_endorsements: Set::<EndorsementId>::with_capacity_and_hasher(
                    pool_settings.max_known_endorsements_size,
                    BuildMap::default(),
                ),
                known_endorsements_queue: VecDeque::with_capacity(
                    pool_settings.max_known_endorsements_size,
                ),
            }
        }

        /// Get bool if block knows about the block and when this information was got
        /// in a option if we don't know if that node knows that block or not
        pub fn get_known_block(&self, block_id: &BlockId) -> Option<&(bool, Instant)> {
            self.known_blocks.get(block_id)
        }

        /// Remove the oldest items from known_blocks
        /// to ensure it contains at most max_node_known_blocks_size items.
        /// This algorithm is optimized for cases where there are no more than a couple excess items, ideally just one.
        fn remove_excess_known_blocks(&mut self, max_node_known_blocks_size: usize) {
            while self.known_blocks.len() > max_node_known_blocks_size {
                // remove oldest item
                let (&h, _) = self
                    .known_blocks
                    .iter()
                    .min_by_key(|(h, (_, t))| (*t, *h))
                    .unwrap(); // never None because is the collection is empty, while loop isn't executed.
                self.known_blocks.remove(&h);
            }
        }

        /// Insert knowledge of a list of blocks in NodeInfo
        ///
        /// ## Arguments
        /// - self: node info
        /// - block_ids: list of blocks
        /// - val: if that node knows that block
        /// - instant: when that information was created
        /// - max_node_known_blocks_size : max size of the knowledge of an other node we want to keep
        pub fn insert_known_blocks(
            &mut self,
            block_ids: &[BlockId],
            val: bool,
            instant: Instant,
            max_node_known_blocks_size: usize,
        ) {
            for block_id in block_ids {
                self.known_blocks.insert(*block_id, (val, instant));
            }
            self.remove_excess_known_blocks(max_node_known_blocks_size);
        }

        pub fn insert_known_endorsements(
            &mut self,
            endorsements: Vec<EndorsementId>,
            max_endorsements_nb: usize,
        ) {
            for endorsement_id in endorsements.into_iter() {
                if self.known_endorsements.insert(endorsement_id) {
                    self.known_endorsements_queue.push_front(endorsement_id);
                    if self.known_endorsements_queue.len() > max_endorsements_nb {
                        if let Some(r) = self.known_endorsements_queue.pop_back() {
                            self.known_endorsements.remove(&r);
                        }
                    }
                }
            }
        }

        pub fn knows_endorsement(&self, endorsement_id: &EndorsementId) -> bool {
            self.known_endorsements.contains(endorsement_id)
        }

        pub fn insert_known_ops(&mut self, ops: Set<OperationId>, max_ops_nb: usize) {
            for operation_id in ops.into_iter() {
                if self.known_operations.insert(operation_id) {
                    self.known_operations_queue.push_front(operation_id);
                    if self.known_operations_queue.len() > max_ops_nb {
                        if let Some(op_id) = self.known_operations_queue.pop_back() {
                            self.known_operations.remove(&op_id);
                        }
                    }
                }
            }
        }

        pub fn knows_op(&self, op: &OperationId) -> bool {
            self.known_operations.contains(op)
        }

        /// Remove the oldest items from wanted_blocks
        /// to ensure it contains at most max_node_wanted_blocks_size items.
        /// This algorithm is optimized for cases where there are no more than a couple excess items, ideally just one.
        fn remove_excess_wanted_blocks(&mut self, max_node_wanted_blocks_size: usize) {
            while self.wanted_blocks.len() > max_node_wanted_blocks_size {
                // remove oldest item
                let (&h, _) = self
                    .wanted_blocks
                    .iter()
                    .min_by_key(|(h, t)| (*t, *h))
                    .unwrap(); // never None because is the collection is empty, while loop isn't executed.
                self.wanted_blocks.remove(&h);
            }
        }

        /// Insert a block in the wanted list of a node.
        /// Also lists the block as not known by the node
        pub fn insert_wanted_block(
            &mut self,
            block_id: BlockId,
            max_node_wanted_blocks_size: usize,
            max_node_known_blocks_size: usize,
        ) {
            // Insert into known_blocks
            let now = Instant::now();
            self.wanted_blocks.insert(block_id, now);
            self.remove_excess_wanted_blocks(max_node_wanted_blocks_size);

            // If the node wants a block, it means that it doesn't have it.
            // To avoid asking the node for this block in the meantime,
            // mark the node as not knowing the block.
            self.insert_known_blocks(&[block_id], false, now, max_node_known_blocks_size);
        }

        /// returns whether a node wants a block, and if so, updates the timestamp of that info to now()
        pub fn contains_wanted_block_update_timestamp(&mut self, block_id: &BlockId) -> bool {
            self.wanted_blocks
                .get_mut(block_id)
                .map(|instant| *instant = Instant::now())
                .is_some()
        }

        /// Removes given block from wanted block for that node
        pub fn remove_wanted_block(&mut self, block_id: &BlockId) -> bool {
            self.wanted_blocks.remove(block_id).is_some()
        }
    }
}

/// Info about a block we've seen
struct BlockInfo {
    /// Endorsements contained in the block header.
    endorsements: Map<EndorsementId, u32>,
    /// Operations contained in the block,
    /// if we've received them already, and none otherwise.
    operations: Option<Vec<OperationId>>,
}

impl BlockInfo {
    fn new(endorsements: Map<EndorsementId, u32>, operations: Option<Vec<OperationId>>) -> Self {
        BlockInfo {
            endorsements,
            operations,
        }
    }
}

pub struct ProtocolWorker {
    /// Protocol configuration.
    protocol_settings: &'static ProtocolSettings,
    /// Operation validity periods
    operation_validity_periods: u64,
    /// Max gas per block
    max_block_gas: u64,
    /// Associated network command sender.
    network_command_sender: NetworkCommandSender,
    /// Associated network event receiver.
    network_event_receiver: NetworkEventReceiver,
    /// Channel to send protocol events to the controller.
    controller_event_tx: mpsc::Sender<ProtocolEvent>,
    /// Channel to send protocol pool events to the controller.
    controller_pool_event_tx: mpsc::Sender<ProtocolPoolEvent>,
    /// Channel receiving commands from the controller.
    controller_command_rx: mpsc::Receiver<ProtocolCommand>,
    /// Channel to send management commands to the controller.
    controller_manager_rx: mpsc::Receiver<ProtocolManagementCommand>,
    /// Ids of active nodes mapped to node info.
    active_nodes: HashMap<NodeId, nodeinfo::NodeInfo>,
    /// List of wanted blocks.
    block_wishlist: Set<BlockId>,
    /// List of processed endorsements
    checked_endorsements: Set<EndorsementId>,
    /// List of processed operations
    checked_operations: Set<OperationId>,
    /// List of processed headers
    checked_headers: Map<BlockId, BlockInfo>,
}

pub struct ProtocolWorkerChannels {
    pub network_command_sender: NetworkCommandSender,
    pub network_event_receiver: NetworkEventReceiver,
    pub controller_event_tx: mpsc::Sender<ProtocolEvent>,
    pub controller_pool_event_tx: mpsc::Sender<ProtocolPoolEvent>,
    pub controller_command_rx: mpsc::Receiver<ProtocolCommand>,
    pub controller_manager_rx: mpsc::Receiver<ProtocolManagementCommand>,
}

impl ProtocolWorker {
    /// Creates a new protocol worker.
    ///
    /// # Arguments
    /// * protocol_settings: protocol configuration.
    /// * operation_validity_periods: operation validity periods
    /// * max_block_gas: max gas per block
    /// * self_node_id: our private key.
    /// * network_controller associated network controller.
    /// * controller_event_tx: Channel to send protocol events.
    /// * controller_command_rx: Channel receiving commands.
    /// * controller_manager_rx: Channel receiving management commands.
    pub fn new(
        protocol_settings: &'static ProtocolSettings,
        operation_validity_periods: u64,
        max_block_gas: u64,
        ProtocolWorkerChannels {
            network_command_sender,
            network_event_receiver,
            controller_event_tx,
            controller_pool_event_tx,
            controller_command_rx,
            controller_manager_rx,
        }: ProtocolWorkerChannels,
    ) -> ProtocolWorker {
        ProtocolWorker {
            protocol_settings,
            operation_validity_periods,
            max_block_gas,
            network_command_sender,
            network_event_receiver,
            controller_event_tx,
            controller_pool_event_tx,
            controller_command_rx,
            controller_manager_rx,
            active_nodes: Default::default(),
            block_wishlist: Default::default(),
            checked_endorsements: Default::default(),
            checked_operations: Default::default(),
            checked_headers: Default::default(),
        }
    }

    async fn send_protocol_event(&self, event: ProtocolEvent) {
        let result = self
            .controller_event_tx
            .send_timeout(event, self.protocol_settings.max_send_wait.to_duration())
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

    async fn send_protocol_pool_event(&self, event: ProtocolPoolEvent) {
        let result = self
            .controller_pool_event_tx
            .send_timeout(event, self.protocol_settings.max_send_wait.to_duration())
            .await;
        match result {
            Ok(()) => {}
            Err(SendTimeoutError::Closed(event)) => {
                warn!(
                    "Failed to send ProtocolPoolEvent due to channel closure: {:?}.",
                    event
                );
            }
            Err(SendTimeoutError::Timeout(event)) => {
                warn!(
                    "Failed to send ProtocolPoolEvent due to timeout: {:?}.",
                    event
                );
            }
        }
    }

    /// Main protocol worker loop. Consumes self.
    /// It is mostly a tokio::select inside a loop
    /// waiting on :
    /// - controller_command_rx
    /// - network_controller
    /// - handshake_futures
    /// - node_event_rx
    /// And at the end every thing is closed properly
    /// Consensus work is managed here.
    /// It's mostly a tokio::select within a loop.
    pub async fn run_loop(mut self) -> Result<NetworkEventReceiver, ProtocolError> {
        let block_ask_timer = sleep(self.protocol_settings.ask_block_timeout.into());
        tokio::pin!(block_ask_timer);
        loop {
            massa_trace!("protocol.protocol_worker.run_loop.begin", {});
            /*
                select! without the "biased" modifier will randomly select the 1st branch to check,
                then will check the next ones in the order they are written.
                We choose this order:
                    * manager commands: low freq, avoid havign to wait to stop
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
                    massa_trace!("protocol.protocol_worker.run_loop.protocol_command_rx", { "cmd": cmd });
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
            } // end select!
            massa_trace!("protocol.protocol_worker.run_loop.end", {});
        } // end loop

        Ok(self.network_event_receiver)
    }

    async fn process_command(
        &mut self,
        cmd: ProtocolCommand,
        timer: &mut std::pin::Pin<&mut Sleep>,
    ) -> Result<(), ProtocolError> {
        massa_trace!("protocol.protocol_worker.process_command.begin", {
            "cmd": cmd
        });
        match cmd {
            ProtocolCommand::IntegratedBlock {
                block_id,
                block,
                operation_ids,
                endorsement_ids,
            } => {
                massa_trace!("protocol.protocol_worker.process_command.integrated_block.begin", { "block_id": block_id, "block": block });
                let now = Instant::now();
                for (node_id, node_info) in self.active_nodes.iter_mut() {
                    // if we know that a node wants a block we send the full block
                    if node_info.remove_wanted_block(&block_id) {
                        node_info.insert_known_blocks(
                            &[block_id],
                            true,
                            now,
                            self.protocol_settings.max_node_known_blocks_size,
                        );
                        node_info.insert_known_endorsements(
                            endorsement_ids.clone(),
                            self.protocol_settings.max_known_endorsements_size,
                        );
                        node_info.insert_known_ops(
                            operation_ids.clone(),
                            self.protocol_settings.max_known_ops_size,
                        );
                        massa_trace!("protocol.protocol_worker.process_command.integrated_block.send_block", { "node": node_id, "block_id": block_id, "block": block });
                        self.network_command_sender
                            .send_block(*node_id, *block.clone())
                            .await
                            .map_err(|_| {
                                ProtocolError::ChannelError(
                                    "send block node command send failed".into(),
                                )
                            })?;
                    } else {
                        // node that isn't asking for that block
                        let cond = node_info.get_known_block(&block_id);
                        // if we don't know if that node knows that hash or if we know it doesn't
                        if !cond.map_or_else(|| false, |v| v.0) {
                            massa_trace!("protocol.protocol_worker.process_command.integrated_block.send_header", { "node": node_id, "block_id": block_id, "header": block.header });
                            self.network_command_sender
                                .send_block_header(*node_id, block.header.clone())
                                .await
                                .map_err(|_| {
                                    ProtocolError::ChannelError(
                                        "send block header network command send failed".into(),
                                    )
                                })?;
                        } else {
                            massa_trace!("protocol.protocol_worker.process_command.integrated_block.do_not_send", { "node": node_id, "block_id": block_id });
                            // Optimization: broadcast block id
                        }
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
            ProtocolCommand::GetBlocksResults(results) => {
                for (block_id, block_info) in results.into_iter() {
                    massa_trace!("protocol.protocol_worker.process_command.found_block.begin", { "block_id": block_id, "block_info": block_info });
                    match block_info {
                        Some((block, opt_operation_ids, opt_endorsement_ids)) => {
                            // Send the block once to all nodes who asked for it.
                            for (node_id, node_info) in self.active_nodes.iter_mut() {
                                if node_info.remove_wanted_block(&block_id) {
                                    node_info.insert_known_blocks(
                                        &[block_id],
                                        true,
                                        Instant::now(),
                                        self.protocol_settings.max_node_known_blocks_size,
                                    );
                                    if let Some(ref endorsement_ids) = opt_endorsement_ids {
                                        // if endorsement IDs are available from the search, note them
                                        // otherwise, it means that they are not relevant anyways (old final block)
                                        node_info.insert_known_endorsements(
                                            endorsement_ids.clone(),
                                            self.protocol_settings.max_known_endorsements_size,
                                        );
                                    }
                                    if let Some(ref operation_ids) = opt_operation_ids {
                                        // if operation IDs are available from the search, note them
                                        // otherwise, it means that they are not relevant anyways (old final block)
                                        node_info.insert_known_ops(
                                            operation_ids.clone(),
                                            self.protocol_settings.max_known_ops_size,
                                        );
                                    }
                                    massa_trace!("protocol.protocol_worker.process_command.found_block.send_block", { "node": node_id, "block_id": block_id, "block": block });
                                    self.network_command_sender
                                        .send_block(*node_id, block.clone())
                                        .await
                                        .map_err(|_| {
                                            ProtocolError::ChannelError(
                                                "send block node command send failed".into(),
                                            )
                                        })?;
                                }
                            }
                            massa_trace!(
                                "protocol.protocol_worker.process_command.found_block.end",
                                {}
                            );
                        }
                        None => {
                            massa_trace!(
                                "protocol.protocol_worker.process_command.block_not_found.begin",
                                { "block_id": block_id }
                            );
                            for (node_id, node_info) in self.active_nodes.iter_mut() {
                                if node_info.contains_wanted_block_update_timestamp(&block_id) {
                                    massa_trace!("protocol.protocol_worker.process_command.block_not_found.notify_node", { "node": node_id, "block_id": block_id });
                                    self.network_command_sender
                                        .block_not_found(*node_id, block_id)
                                        .await?
                                }
                            }
                            massa_trace!(
                                "protocol.protocol_worker.process_command.block_not_found.end",
                                {}
                            );
                        }
                    }
                }
            }
            ProtocolCommand::WishlistDelta { new, remove } => {
                massa_trace!("protocol.protocol_worker.process_command.wishlist_delta.begin", { "new": new, "remove": remove });
                self.stop_asking_blocks(remove)?;
                self.block_wishlist.extend(new);
                self.update_ask_block(timer).await?;
                massa_trace!(
                    "protocol.protocol_worker.process_command.wishlist_delta.end",
                    {}
                );
            }
            ProtocolCommand::PropagateOperations(ops) => {
                massa_trace!(
                    "protocol.protocol_worker.process_command.propagate_operations.begin",
                    { "operations": ops }
                );
                for (node, node_info) in self.active_nodes.iter_mut() {
                    let new_ops: Map<OperationId, Operation> = ops
                        .iter()
                        .filter(|(id, _)| !node_info.knows_op(*id))
                        .map(|(k, v)| (*k, v.clone()))
                        .collect();
                    node_info.insert_known_ops(
                        new_ops.keys().copied().collect(),
                        self.protocol_settings.max_known_ops_size,
                    );
                    let to_send = new_ops.into_iter().map(|(_, op)| op).collect::<Vec<_>>();
                    if !to_send.is_empty() {
                        self.network_command_sender
                            .send_operations(*node, to_send)
                            .await?;
                    }
                }
            }
            ProtocolCommand::PropagateEndorsements(endorsements) => {
                massa_trace!(
                    "protocol.protocol_worker.process_command.propagate_endorsements.begin",
                    { "endorsements": endorsements }
                );
                for (node, node_info) in self.active_nodes.iter_mut() {
                    let new_endorsements: Map<EndorsementId, Endorsement> = endorsements
                        .iter()
                        .filter(|(id, _)| !node_info.knows_endorsement(*id))
                        .map(|(k, v)| (*k, v.clone()))
                        .collect();
                    node_info.insert_known_endorsements(
                        new_endorsements.keys().copied().collect(),
                        self.protocol_settings.max_known_endorsements_size,
                    );
                    let to_send = new_endorsements
                        .into_iter()
                        .map(|(_, op)| op)
                        .collect::<Vec<_>>();
                    if !to_send.is_empty() {
                        self.network_command_sender
                            .send_endorsements(*node, to_send)
                            .await?;
                    }
                }
            }
        }
        massa_trace!("protocol.protocol_worker.process_command.end", {});
        Ok(())
    }

    fn stop_asking_blocks(&mut self, remove_hashes: Set<BlockId>) -> Result<(), ProtocolError> {
        massa_trace!("protocol.protocol_worker.stop_asking_blocks", {
            "remove": remove_hashes
        });
        for node_info in self.active_nodes.values_mut() {
            node_info
                .asked_blocks
                .retain(|h, _| !remove_hashes.contains(h));
        }
        self.block_wishlist.retain(|h| !remove_hashes.contains(h));
        Ok(())
    }

    async fn update_ask_block(
        &mut self,
        ask_block_timer: &mut std::pin::Pin<&mut Sleep>,
    ) -> Result<(), ProtocolError> {
        massa_trace!("protocol.protocol_worker.update_ask_block.begin", {});

        let now = Instant::now();

        // init timer
        let mut next_tick = now
            .checked_add(self.protocol_settings.ask_block_timeout.into())
            .ok_or(TimeError::TimeOverflowError)?;

        // list blocks to re-ask and gather candidate nodes to ask from
        let mut candidate_nodes: Map<BlockId, Vec<_>> = Default::default();
        let mut ask_block_list: HashMap<NodeId, Vec<BlockId>> = Default::default();

        // list blocks to re-ask and from whom
        for hash in self.block_wishlist.iter() {
            let mut needs_ask = true;

            for (node_id, node_info) in self.active_nodes.iter_mut() {
                // map to remove the borrow on asked_blocks. Otherwise can't call insert_known_blocks
                let ask_time_opt = node_info.asked_blocks.get(hash).copied();
                let (timeout_at_opt, timed_out) = if let Some(ask_time) = ask_time_opt {
                    let t = ask_time
                        .checked_add(self.protocol_settings.ask_block_timeout.into())
                        .ok_or(TimeError::TimeOverflowError)?;
                    (Some(t), t <= now)
                } else {
                    (None, false)
                };
                let knows_block = node_info.get_known_block(hash);

                // check if the node recently told us it doesn't have the block
                if let Some((false, info_time)) = knows_block {
                    let info_expires = info_time
                        .checked_add(self.protocol_settings.ask_block_timeout.into())
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
                                self.protocol_settings.max_node_known_blocks_size,
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
                                self.protocol_settings.max_node_known_blocks_size,
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
                            self.protocol_settings.max_node_known_blocks_size,
                        );
                        (2u8, ask_time_opt)
                    }
                };

                // add candidate node
                candidate_nodes
                    .entry(*hash)
                    .or_insert_with(Vec::new)
                    .push((candidate, *node_id));
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
                                .checked_add(self.protocol_settings.ask_block_timeout.into())
                                .map_or(false, |timeout_t| timeout_t > now)
                        })
                        .count(),
                )
            })
            .collect();

        for (hash, criteria) in candidate_nodes.into_iter() {
            // find the best node
            if let Some((_knowledge, best_node)) = criteria
                .into_iter()
                .filter(|(_knowledge, node_id)| {
                    // filter out nodes with too many active block requests
                    *active_block_req_count.get(node_id).unwrap_or(&0)
                        <= self.protocol_settings.max_simultaneous_ask_blocks_per_node
                })
                .min_by_key(|(knowledge, node_id)| {
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
                    .push(hash);

                let timeout_at = now
                    .checked_add(self.protocol_settings.ask_block_timeout.into())
                    .ok_or(TimeError::TimeOverflowError)?;
                next_tick = std::cmp::min(next_tick, timeout_at);
            }
        }

        // send AskBlockEvents
        if !ask_block_list.is_empty() {
            massa_trace!("protocol.protocol_worker.update_ask_block", {
                "list": ask_block_list
            });
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
    async fn ban_node(&mut self, node_id: &NodeId) -> Result<(), ProtocolError> {
        massa_trace!("protocol.protocol_worker.ban_node", { "node": node_id });
        self.active_nodes.remove(node_id);
        self.network_command_sender
            .ban(*node_id)
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
    /// - Can compute a BlockId.
    /// - Valid signature.
    /// - Absence of duplicate endorsements.
    ///
    /// Checks performed on endorsements:
    /// - Unique indices.
    /// - Slot matches that of the block.
    /// - Block matches that of the block.
    async fn note_header_from_node(
        &mut self,
        header: &BlockHeader,
        source_node_id: &NodeId,
    ) -> Result<Option<(BlockId, Map<EndorsementId, u32>, bool)>, ProtocolError> {
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
        let block_id = match header.compute_block_id() {
            Ok(id) => id,
            Err(err) => {
                massa_trace!("protocol.protocol_worker.check_header.err_id", { "header": header, "err": format!("{}", err)});
                return Ok(None);
            }
        };

        // check if this header was already verified
        let now = Instant::now();
        if let Some(block_info) = self.checked_headers.get(&block_id) {
            if let Some(node_info) = self.active_nodes.get_mut(source_node_id) {
                node_info.insert_known_blocks(
                    &header.content.parents,
                    true,
                    now,
                    self.protocol_settings.max_node_known_blocks_size,
                );
                node_info.insert_known_blocks(
                    &[block_id],
                    true,
                    now,
                    self.protocol_settings.max_node_known_blocks_size,
                );
                node_info.insert_known_endorsements(
                    block_info.endorsements.keys().copied().collect(),
                    self.protocol_settings.max_known_endorsements_size,
                );
                if let Some(operations) = block_info.operations.as_ref() {
                    node_info.insert_known_ops(
                        operations.iter().cloned().collect(),
                        self.protocol_settings.max_known_ops_size,
                    );
                }
            }
            return Ok(Some((block_id, block_info.endorsements.clone(), false)));
        }

        let (endorsement_ids, endorsements_reused) = match self
            .note_endorsements_from_node(header.content.endorsements.clone(), source_node_id, false)
            .await
        {
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
        if let Err(err) = header.check_signature() {
            massa_trace!("protocol.protocol_worker.check_header.err_signature", { "header": header, "err": format!("{}", err)});
            return Ok(None);
        };

        // check endorsement in header integrity
        let mut used_endorsement_indices: HashSet<u32> =
            HashSet::with_capacity(header.content.endorsements.len());
        for endorsement in header.content.endorsements.iter() {
            // check index reuse
            if !used_endorsement_indices.insert(endorsement.content.index) {
                massa_trace!("protocol.protocol_worker.check_header.err_endorsement_index_reused", { "header": header, "edorsement": endorsement});
                return Ok(None);
            }
            // check slot
            if (endorsement.content.slot.thread != header.content.slot.thread)
                || (endorsement.content.slot >= header.content.slot)
            {
                massa_trace!("protocol.protocol_worker.check_header.err_endorsement_invalid_slot", { "header": header, "edorsement": endorsement});
                return Ok(None);
            }
            // check endorsed block
            if endorsement.content.endorsed_block
                != header.content.parents[header.content.slot.thread as usize]
            {
                massa_trace!("protocol.protocol_worker.check_header.err_endorsement_invalid_endorsed_block", { "header": header, "edorsement": endorsement});
                return Ok(None);
            }
        }

        if self
            .checked_headers
            .insert(
                block_id,
                BlockInfo::new(endorsement_ids.clone(), Default::default()),
            )
            .is_none()
        {
            self.prune_checked_headers();
        }

        if let Some(node_info) = self.active_nodes.get_mut(source_node_id) {
            node_info.insert_known_blocks(
                &header.content.parents,
                true,
                now,
                self.protocol_settings.max_node_known_blocks_size,
            );
            node_info.insert_known_blocks(
                &[block_id],
                true,
                now,
                self.protocol_settings.max_node_known_blocks_size,
            );
            massa_trace!("protocol.protocol_worker.note_header_from_node.ok", { "node": source_node_id,"block_id":block_id, "header": header});
            return Ok(Some((block_id, endorsement_ids, true)));
        }
        Ok(None)
    }

    /// Prune checked_endorsements if it is too large
    fn prune_checked_endorsements(&mut self) {
        if self.checked_endorsements.len() > self.protocol_settings.max_known_endorsements_size {
            self.checked_endorsements.clear();
        }
    }

    /// Prune checked operations if it has grown too large.
    fn prune_checked_operations(&mut self) {
        if self.checked_operations.len() > self.protocol_settings.max_known_ops_size {
            self.checked_operations.clear();
        }
    }

    /// Prune checked_headers if it is too large
    fn prune_checked_headers(&mut self) {
        if self.checked_headers.len() > self.protocol_settings.max_node_known_blocks_size {
            self.checked_headers.clear();
        }
    }

    /// Check a header's signature, and if valid note the node knows the block.
    /// Does not ban if the block is invalid.
    ///
    /// Checks performed:
    /// - Check the header(see note_header_from_node).
    /// - Check operations(see note_operations_from_node).
    /// - Check operations:
    ///     - Absense of duplicates.
    ///     - Validity period includes the slot of the block.
    ///     - Address matches that of the block.
    ///     - Thread matches that of the block.
    /// - Check root hash.
    async fn note_block_from_node(
        &mut self,
        block: &Block,
        source_node_id: &NodeId,
    ) -> Result<
        Option<(
            BlockId,
            Map<OperationId, (usize, u64)>,
            Map<EndorsementId, u32>,
        )>,
        ProtocolError,
    > {
        massa_trace!("protocol.protocol_worker.note_block_from_node", { "node": source_node_id, "block": block });

        // check header
        let (block_id, endorsement_ids, _is_header_new) = match self
            .note_header_from_node(&block.header, source_node_id)
            .await
        {
            Ok(Some(v)) => v,
            Ok(None) => return Ok(None),
            Err(err) => return Err(err),
        };

        let serialization_context =
            massa_models::with_serialization_context(|context| context.clone());

        // Perform general checks on the operations, note them into caches and send them to pool
        // but do not propagate as they are already propagating within a block
        let (seen_ops, received_operations_ids, has_duplicate_operations, total_gas) = self
            .note_operations_from_node(block.operations.clone(), source_node_id, false)
            .await?;
        if total_gas > self.max_block_gas {
            // Gas usage over limit => block invalid
            // TODO remove this check in the single-ledger version,
            //      this is only here to prevent ExecuteSC senders from spending gas fees while the block is unable to execute their op
            return Ok(None);
        }

        // Perform checks on the operations that relate to the block in which they have been included.
        // We perform those checks AFTER note_operations_from_node to allow otherwise valid operations to be noted
        if has_duplicate_operations {
            // Block contains duplicate operations.
            return Ok(None);
        }
        for op in block.operations.iter() {
            // check validity period
            if !(op
                .get_validity_range(self.operation_validity_periods)
                .contains(&block.header.content.slot.period))
            {
                massa_trace!("protocol.protocol_worker.note_block_from_node.err_op_period",
                    { "node": source_node_id,"block_id":block_id, "block": block, "op": op });
                return Ok(None);
            }

            // check address and thread
            let addr = Address::from_public_key(&op.content.sender_public_key);
            if addr.get_thread(serialization_context.thread_count)
                != block.header.content.slot.thread
            {
                massa_trace!("protocol.protocol_worker.note_block_from_node.err_op_thread",
                    { "node": source_node_id,"block_id":block_id, "block": block, "op": op});
                return Ok(None);
            }
        }

        // check root hash
        {
            let concat_bytes = seen_ops
                .iter()
                .map(|op_id| op_id.to_bytes().to_vec())
                .concat();
            if block.header.content.operation_merkle_root != Hash::compute_from(&concat_bytes) {
                massa_trace!("protocol.protocol_worker.note_block_from_node.err_op_root_hash",
                    { "node": source_node_id,"block_id":block_id, "block": block });
                return Ok(None);
            }
        }

        // Add operations to block info if found in the cache.
        if let Some(mut block_info) = self.checked_headers.get_mut(&block_id) {
            if block_info.operations.is_none() {
                block_info.operations = Some(received_operations_ids.keys().cloned().collect());
            }
        }

        // Note: block already added to node's view in `note_header_from_node`.

        Ok(Some((block_id, received_operations_ids, endorsement_ids)))
    }

    /// Checks operations, caching knowledge of valid ones.
    ///
    /// Does not ban if the operation is invalid.
    ///
    /// Returns :
    /// - a list of seen operation ids, for use in checking the root hash of the block.
    /// - a map of seen operations with indices and validity periods to avoid recomputing them later
    /// - a boolean indicating whether duplicate operations were noted.
    /// - the sum of all operation's max_gas.
    ///
    /// Checks performed:
    /// - Valid signature
    async fn note_operations_from_node(
        &mut self,
        operations: Vec<Operation>,
        source_node_id: &NodeId,
        propagate: bool,
    ) -> Result<(Vec<OperationId>, Map<OperationId, (usize, u64)>, bool, u64), ProtocolError> {
        massa_trace!("protocol.protocol_worker.note_operations_from_node", { "node": source_node_id, "operations": operations });
        let mut total_gas = 0u64;
        let length = operations.len();
        let mut has_duplicate_operations = false;
        let mut seen_ops = vec![];
        let mut new_operations = Map::with_capacity_and_hasher(length, BuildMap::default());
        let mut received_ids = Map::with_capacity_and_hasher(length, BuildMap::default());
        for (idx, operation) in operations.into_iter().enumerate() {
            let operation_id = operation.get_operation_id()?;
            seen_ops.push(operation_id);

            // Note: we always want to update the node's view of known operations,
            // even if we cached the check previously.
            let was_present =
                received_ids.insert(operation_id, (idx, operation.content.expire_period));

            // There are duplicate operations in this batch.
            if was_present.is_some() {
                has_duplicate_operations = true;
            }

            // Accumulate gas
            if let OperationType::ExecuteSC { max_gas, .. } = &operation.content.op {
                total_gas = total_gas.saturating_add(*max_gas);
            }

            // Check operation signature only if not already checked.
            if self.checked_operations.insert(operation_id) {
                // check signature
                operation.verify_signature()?;
                new_operations.insert(operation_id, operation);
            };
        }

        // add to known ops
        if let Some(node_info) = self.active_nodes.get_mut(source_node_id) {
            node_info.insert_known_ops(
                received_ids.keys().copied().collect(),
                self.protocol_settings.max_known_ops_size,
            );
        }

        if !new_operations.is_empty() {
            // Add to pool, propagate when received outside of a header.
            self.send_protocol_pool_event(ProtocolPoolEvent::ReceivedOperations {
                operations: new_operations,
                propagate,
            })
            .await;

            // prune checked operations cache
            self.prune_checked_operations();
        }

        Ok((seen_ops, received_ids, has_duplicate_operations, total_gas))
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
    async fn note_endorsements_from_node(
        &mut self,
        endorsements: Vec<Endorsement>,
        source_node_id: &NodeId,
        propagate: bool,
    ) -> Result<(Map<EndorsementId, u32>, bool), ProtocolError> {
        massa_trace!("protocol.protocol_worker.note_endorsements_from_node", { "node": source_node_id, "endorsements": endorsements});
        let length = endorsements.len();
        let mut contains_duplicates = false;

        let mut new_endorsements = Map::with_capacity_and_hasher(length, BuildMap::default());
        let mut endorsement_ids = Map::default();
        for endorsement in endorsements.into_iter() {
            let endorsement_id = endorsement.compute_endorsement_id()?;
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
                self.protocol_settings.max_known_endorsements_size,
            );
        }

        if !new_endorsements.is_empty() {
            self.prune_checked_endorsements();

            // Add to pool, propagate if required
            self.send_protocol_pool_event(ProtocolPoolEvent::ReceivedEndorsements {
                endorsements: new_endorsements,
                propagate,
            })
            .await;
        }

        Ok((endorsement_ids, contains_duplicates))
    }

    /// Manages network event
    /// Only used by the worker.
    ///
    /// # Argument
    /// evt: event to process
    async fn on_network_event(
        &mut self,
        evt: NetworkEvent,
        block_ask_timer: &mut std::pin::Pin<&mut Sleep>,
    ) -> Result<(), ProtocolError> {
        match evt {
            NetworkEvent::NewConnection(node_id) => {
                info!("Connected to node {}", node_id);
                massa_trace!(
                    "protocol.protocol_worker.on_network_event.new_connection",
                    { "node": node_id }
                );
                self.active_nodes
                    .insert(node_id, nodeinfo::NodeInfo::new(self.protocol_settings));
                self.update_ask_block(block_ask_timer).await?;
            }
            NetworkEvent::ConnectionClosed(node_id) => {
                massa_trace!(
                    "protocol.protocol_worker.on_network_event.connection_closed",
                    { "node": node_id }
                );
                if self.active_nodes.remove(&node_id).is_some() {
                    // deletes all node info
                    info!("Connection closed with {}", node_id);
                    self.update_ask_block(block_ask_timer).await?;
                }
            }
            NetworkEvent::ReceivedBlock {
                node: from_node_id,
                block,
            } => {
                massa_trace!("protocol.protocol_worker.on_network_event.received_block", { "node": from_node_id, "block": block});
                if let Some((block_id, operation_set, endorsement_ids)) =
                    self.note_block_from_node(&block, &from_node_id).await?
                {
                    let mut set = Set::<BlockId>::with_capacity_and_hasher(1, BuildMap::default());
                    set.insert(block_id);
                    self.stop_asking_blocks(set)?;
                    self.send_protocol_event(ProtocolEvent::ReceivedBlock {
                        block_id,
                        block,
                        operation_set,
                        endorsement_ids,
                    })
                    .await;
                    self.update_ask_block(block_ask_timer).await?;
                } else {
                    warn!("node {} sent us critically incorrect block, which may be an attack attempt by the remote node or a loss of sync between us and the remote node", from_node_id);
                    let _ = self.ban_node(&from_node_id).await;
                }
            }
            NetworkEvent::AskedForBlocks {
                node: from_node_id,
                list,
            } => {
                massa_trace!("protocol.protocol_worker.on_network_event.asked_for_blocks", { "node": from_node_id, "hashlist": list});
                if let Some(node_info) = self.active_nodes.get_mut(&from_node_id) {
                    for hash in &list {
                        node_info.insert_wanted_block(
                            *hash,
                            self.protocol_settings.max_node_wanted_blocks_size,
                            self.protocol_settings.max_node_known_blocks_size,
                        );
                    }
                } else {
                    return Ok(());
                }
                self.send_protocol_event(ProtocolEvent::GetBlocks(list))
                    .await;
            }
            NetworkEvent::ReceivedBlockHeader {
                source_node_id,
                header,
            } => {
                massa_trace!("protocol.protocol_worker.on_network_event.received_block_header", { "node": source_node_id, "header": header});
                if let Some((block_id, _endorsement_ids, is_new)) =
                    self.note_header_from_node(&header, &source_node_id).await?
                {
                    if is_new {
                        self.send_protocol_event(ProtocolEvent::ReceivedBlockHeader {
                            block_id,
                            header,
                        })
                        .await;
                    }
                    self.update_ask_block(block_ask_timer).await?;
                } else {
                    warn!(
                        "node {} sent us critically incorrect header, which may be an attack attempt by the remote node or a loss of sync between us and the remote node",
                        source_node_id,
                    );
                    let _ = self.ban_node(&source_node_id).await;
                }
            }
            NetworkEvent::BlockNotFound { node, block_id } => {
                massa_trace!("protocol.protocol_worker.on_network_event.block_not_found", { "node": node, "block_id": block_id});
                if let Some(info) = self.active_nodes.get_mut(&node) {
                    info.insert_known_blocks(
                        &[block_id],
                        false,
                        Instant::now(),
                        self.protocol_settings.max_node_known_blocks_size,
                    );
                }
                self.update_ask_block(block_ask_timer).await?;
            }
            NetworkEvent::ReceivedOperations { node, operations } => {
                massa_trace!("protocol.protocol_worker.on_network_event.received_operations", { "node": node, "operations": operations});

                // Perform general checks on the operations, and propagate them.
                if self
                    .note_operations_from_node(operations, &node, true)
                    .await
                    .is_err()
                {
                    warn!("node {} sent us critically incorrect operation, which may be an attack attempt by the remote node or a loss of sync between us and the remote node", node,);
                    let _ = self.ban_node(&node).await;
                }
            }
            NetworkEvent::ReceivedEndorsements { node, endorsements } => {
                massa_trace!("protocol.protocol_worker.on_network_event.received_endorsements", { "node": node, "endorsements": endorsements});
                if self
                    .note_endorsements_from_node(endorsements, &node, true)
                    .await
                    .is_err()
                {
                    warn!("node {} sent us critically incorrect endorsements, which may be an attack attempt by the remote node or a loss of sync between us and the remote node", node,);
                    let _ = self.ban_node(&node).await;
                }
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::nodeinfo::NodeInfo;
    use super::*;
    use massa_protocol_exports::tests::tools::create_protocol_settings;
    use serial_test::serial;

    lazy_static::lazy_static! {
        static ref PROTOCOL_SETTINGS: ProtocolSettings = create_protocol_settings();
    }

    pub fn get_dummy_block_id(s: &str) -> BlockId {
        BlockId(Hash::compute_from(s.as_bytes()))
    }

    /// Test the pruning behavior of NodeInfo::insert_wanted_block
    #[test]
    #[serial]
    fn test_node_info_wanted_blocks_pruning() {
        let protocol_settings = &PROTOCOL_SETTINGS;
        let mut nodeinfo = NodeInfo::new(protocol_settings);

        // cap to 10 wanted blocks
        let max_node_wanted_blocks_size = 10;

        // cap to 5 known blocks
        let max_node_known_blocks_size = 5;

        // try to insert 15 wanted blocks
        for index in 0usize..15 {
            let hash = get_dummy_block_id(&index.to_string());
            nodeinfo.insert_wanted_block(
                hash,
                max_node_wanted_blocks_size,
                max_node_known_blocks_size,
            );
        }

        // ensure that only max_node_wanted_blocks_size wanted blocks are kept
        assert_eq!(
            nodeinfo.wanted_blocks.len(),
            max_node_wanted_blocks_size,
            "wanted_blocks pruning incorrect"
        );

        // ensure that there are max_node_known_blocks_size entries for knwon blocks
        assert_eq!(
            nodeinfo.known_blocks.len(),
            max_node_known_blocks_size,
            "known_blocks pruning incorrect"
        );
    }

    #[test]
    #[serial]
    fn test_node_info_know_block() {
        let max_node_known_blocks_size = 10;
        let protocol_settings = &PROTOCOL_SETTINGS;
        let mut nodeinfo = NodeInfo::new(protocol_settings);
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

    #[test]
    #[serial]
    fn test_node_info_wanted_block() {
        let max_node_wanted_blocks_size = 10;
        let max_node_known_blocks_size = 10;
        let protocol_settings = &PROTOCOL_SETTINGS;
        let mut nodeinfo = NodeInfo::new(protocol_settings);

        let hash = get_dummy_block_id("test");
        nodeinfo.insert_wanted_block(
            hash,
            max_node_wanted_blocks_size,
            max_node_known_blocks_size,
        );
        assert!(nodeinfo.contains_wanted_block_update_timestamp(&hash));
        nodeinfo.remove_wanted_block(&hash);
        assert!(!nodeinfo.contains_wanted_block_update_timestamp(&hash));

        for index in 0..9 {
            let hash = get_dummy_block_id(&index.to_string());
            nodeinfo.insert_wanted_block(
                hash,
                max_node_wanted_blocks_size,
                max_node_known_blocks_size,
            );
            assert!(nodeinfo.contains_wanted_block_update_timestamp(&hash));
        }

        // change the oldest time to now
        assert!(
            nodeinfo.contains_wanted_block_update_timestamp(&get_dummy_block_id(&0.to_string()))
        );
        // add hash that triggers container pruning
        nodeinfo.insert_wanted_block(
            get_dummy_block_id("test2"),
            max_node_wanted_blocks_size,
            max_node_known_blocks_size,
        );

        // 0 is present because because its timestamp has been updated with contains_wanted_block
        assert!(
            nodeinfo.contains_wanted_block_update_timestamp(&get_dummy_block_id(&0.to_string()))
        );

        // 1 has been removed because it's the oldest.
        assert!(
            nodeinfo.contains_wanted_block_update_timestamp(&get_dummy_block_id(&1.to_string()))
        );

        // Other blocks are present.
        for index in 2..9 {
            let hash = get_dummy_block_id(&index.to_string());
            assert!(nodeinfo.contains_wanted_block_update_timestamp(&hash));
        }
    }
}
