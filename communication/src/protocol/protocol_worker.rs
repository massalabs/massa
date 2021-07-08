use super::config::ProtocolConfig;
use crate::common::NodeId;
use crate::error::CommunicationError;
use crate::network::{NetworkCommandSender, NetworkEvent, NetworkEventReceiver};
use crypto::hash::Hash;
use itertools::Itertools;
use models::{Address, Block, BlockHeader, BlockId, Operation, OperationId, SerializationContext};
use serde::Serialize;
use std::collections::{HashMap, HashSet};
use time::TimeError;
use tokio::{
    sync::mpsc,
    sync::mpsc::error::SendTimeoutError,
    time::{sleep, sleep_until, Duration, Instant, Sleep},
};

/// Possible types of events that can happen.
#[derive(Debug, Serialize)]
pub enum ProtocolEvent {
    /// A block with a valid signature has been received.
    ReceivedBlock {
        block_id: BlockId,
        block: Block,
        operation_set: HashMap<OperationId, usize>,
    },
    /// A block header with a valid signature has been received.
    ReceivedBlockHeader {
        block_id: BlockId,
        header: BlockHeader,
    },
    /// Ask for a list of blocks from consensus.
    GetBlocks(Vec<BlockId>),
}
/// Possible types of pool events that can happen.
#[derive(Debug, Serialize)]
pub enum ProtocolPoolEvent {
    /// Operations were received
    ReceivedOperations(HashMap<OperationId, Operation>),
}

/// Commands that protocol worker can process
#[derive(Debug, Serialize)]
pub enum ProtocolCommand {
    /// Notify block integration of a given block.
    IntegratedBlock { block_id: BlockId, block: Block },
    /// A block, or it's header, amounted to an attempted attack.
    AttackBlockDetected(BlockId),
    /// Wishlist delta
    WishlistDelta {
        new: HashSet<BlockId>,
        remove: HashSet<BlockId>,
    },
    /// The response to a ProtocolEvent::GetBlocks.
    GetBlocksResults(HashMap<BlockId, Option<Block>>),
    /// Propagate operations
    PropagateOperations(HashMap<OperationId, Operation>),
}

#[derive(Debug, Serialize)]
pub enum ProtocolManagementCommand {}

//put in a module to block private access from Protocol_worker.
mod nodeinfo {
    use models::BlockId;
    use std::collections::HashMap;
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
        known_blocks: HashMap<BlockId, (bool, Instant)>,
        /// The blocks the node asked for.
        wanted_blocks: HashMap<BlockId, Instant>,
        /// Blocks we asked that node for
        pub asked_blocks: HashMap<BlockId, Instant>,
        /// Instant when the node was added
        pub connection_instant: Instant,
    }

    impl NodeInfo {
        pub fn new() -> NodeInfo {
            NodeInfo {
                known_blocks: HashMap::new(),
                wanted_blocks: HashMap::new(),
                asked_blocks: HashMap::new(),
                connection_instant: Instant::now(),
            }
        }

        pub fn get_known_block(&self, block_id: &BlockId) -> Option<&(bool, Instant)> {
            self.known_blocks.get(block_id)
        }

        pub fn insert_known_block(
            &mut self,
            block_id: BlockId,
            val: bool,
            instant: Instant,
            max_node_known_blocks_size: usize,
        ) {
            self.known_blocks.insert(block_id, (val, instant));
            while self.known_blocks.len() > max_node_known_blocks_size {
                //remove oldest item
                let (&h, _) = self
                    .known_blocks
                    .iter()
                    .min_by_key(|(h, (_, t))| (*t, *h))
                    .unwrap(); //never None because is the collection is empty, while loop isn't executed.
                self.known_blocks.remove(&h);
            }
        }

        pub fn insert_wanted_blocks(
            &mut self,
            block_id: BlockId,
            max_node_wanted_blocks_size: usize,
        ) {
            self.wanted_blocks.insert(block_id, Instant::now());
            while self.known_blocks.len() > max_node_wanted_blocks_size {
                //remove oldest item
                let (&h, _) = self
                    .known_blocks
                    .iter()
                    .min_by_key(|(h, t)| (*t, *h))
                    .unwrap(); //never None because is the collection is empty, while loop isn't executed.
                self.known_blocks.remove(&h);
            }
        }

        pub fn contains_wanted_block(&mut self, block_id: &BlockId) -> bool {
            self.wanted_blocks
                .get_mut(block_id)
                .map(|instant| *instant = Instant::now())
                .is_some()
        }

        pub fn remove_wanted_block(&mut self, block_id: &BlockId) -> bool {
            self.wanted_blocks.remove(block_id).is_some()
        }
    }
}

pub struct ProtocolWorker {
    /// Protocol configuration.
    cfg: ProtocolConfig,
    /// Operation validity periods
    operation_validity_periods: u64,
    // Serialization context
    serialization_context: SerializationContext,
    /// Associated nework command sender.
    network_command_sender: NetworkCommandSender,
    /// Associated nework event receiver.
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
    block_wishlist: HashSet<BlockId>,
}

impl ProtocolWorker {
    /// Creates a new protocol worker.
    ///
    /// # Arguments
    /// * cfg: protocol configuration.
    /// * operation_validity_periods: operation validity periods
    /// * self_node_id: our private key.
    /// * network_controller associated network controller.
    /// * controller_event_tx: Channel to send protocol events.
    /// * controller_command_rx: Channel receiving commands.
    /// * controller_manager_rx: Channel receiving management commands.
    pub fn new(
        cfg: ProtocolConfig,
        operation_validity_periods: u64,
        serialization_context: SerializationContext,
        network_command_sender: NetworkCommandSender,
        network_event_receiver: NetworkEventReceiver,
        controller_event_tx: mpsc::Sender<ProtocolEvent>,
        controller_pool_event_tx: mpsc::Sender<ProtocolPoolEvent>,
        controller_command_rx: mpsc::Receiver<ProtocolCommand>,
        controller_manager_rx: mpsc::Receiver<ProtocolManagementCommand>,
    ) -> ProtocolWorker {
        ProtocolWorker {
            serialization_context,
            cfg,
            operation_validity_periods,
            network_command_sender,
            network_event_receiver,
            controller_event_tx,
            controller_pool_event_tx,
            controller_command_rx,
            controller_manager_rx,
            active_nodes: HashMap::new(),
            block_wishlist: HashSet::new(),
        }
    }

    async fn send_protocol_event(&self, event: ProtocolEvent) {
        let result = self
            .controller_event_tx
            .send_timeout(
                event,
                Duration::from_millis(self.cfg.max_send_wait.to_millis()),
            )
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
            .send_timeout(
                event,
                Duration::from_millis(self.cfg.max_send_wait.to_millis()),
            )
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
    /// wainting on :
    /// - controller_command_rx
    /// - network_controller
    /// - handshake_futures
    /// - node_event_rx
    /// And at the end every thing is closed properly
    /// Consensus work is managed here.
    /// It's mostly a tokio::select within a loop.
    pub async fn run_loop(mut self) -> Result<NetworkEventReceiver, CommunicationError> {
        let block_ask_timer = sleep(self.cfg.ask_block_timeout.into());
        tokio::pin!(block_ask_timer);
        loop {
            massa_trace!("protocol.protocol_worker.run_loop.begin", {});
            tokio::select! {
                // block ask timer
                _ = &mut block_ask_timer => {
                    massa_trace!("protocol.protocol_worker.run_loop.block_ask_timer", { });
                    self.update_ask_block(&mut block_ask_timer).await?;
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

                // listen to management commands
                cmd = self.controller_manager_rx.recv() => {
                    massa_trace!("protocol.protocol_worker.run_loop.controller_manager_rx", { "cmd": cmd });
                    match cmd {
                        None => break,
                        Some(_) => {}
                    };
                }
            } //end select!
            massa_trace!("protocol.protocol_worker.run_loop.end", {});
        } //end loop

        Ok(self.network_event_receiver)
    }

    async fn process_command(
        &mut self,
        cmd: ProtocolCommand,
        timer: &mut std::pin::Pin<&mut Sleep>,
    ) -> Result<(), CommunicationError> {
        massa_trace!("protocol.protocol_worker.process_command.begin", {
            "cmd": cmd
        });
        match cmd {
            ProtocolCommand::IntegratedBlock { block_id, block } => {
                massa_trace!("protocol.protocol_worker.process_command.integrated_block.begin", { "block_id": block_id, "block": block });
                for (node_id, node_info) in self.active_nodes.iter_mut() {
                    // if we know that a node wants a block we send the full block
                    if node_info.remove_wanted_block(&block_id) {
                        node_info.insert_known_block(
                            block_id,
                            true,
                            Instant::now(),
                            self.cfg.max_node_known_blocks_size,
                        );
                        massa_trace!("protocol.protocol_worker.process_command.integrated_block.send_block", { "node": node_id, "block_id": block_id, "block": block });
                        self.network_command_sender
                            .send_block(*node_id, block.clone())
                            .await
                            .map_err(|_| {
                                CommunicationError::ChannelError(
                                    "send block node command send failed".into(),
                                )
                            })?;
                    } else {
                        // node that aren't asking for that block
                        let cond = node_info.get_known_block(&block_id);
                        // if we don't know if that node knowns that hash or if we know it doesn't
                        if !cond.map_or_else(|| false, |v| v.0) {
                            massa_trace!("protocol.protocol_worker.process_command.integrated_block.send_header", { "node": node_id, "block_id": block_id, "header": block.header });
                            self.network_command_sender
                                .send_block_header(*node_id, block.header.clone())
                                .await
                                .map_err(|_| {
                                    CommunicationError::ChannelError(
                                        "send block header network command send failed".into(),
                                    )
                                })?;
                        } else {
                            massa_trace!("protocol.protocol_worker.process_command.integrated_block.do_not_send", { "node": node_id, "block_id": block_id });
                            // todo broadcast hash (see #202)
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
                        Some((true, _)) => Some(id.clone()),
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
                for (block_id, block) in results.into_iter() {
                    massa_trace!("protocol.protocol_worker.process_command.found_block.begin", { "block_id": block_id, "block": block });
                    match block {
                        Some(block) => {
                            // Send the block once to all nodes who asked for it.
                            for (node_id, node_info) in self.active_nodes.iter_mut() {
                                if node_info.remove_wanted_block(&block_id) {
                                    node_info.insert_known_block(
                                        block_id,
                                        true,
                                        Instant::now(),
                                        self.cfg.max_node_known_blocks_size,
                                    );
                                    massa_trace!("protocol.protocol_worker.process_command.found_block.send_block", { "node": node_id, "block_id": block_id, "block": block });
                                    self.network_command_sender
                                        .send_block(*node_id, block.clone())
                                        .await
                                        .map_err(|_| {
                                            CommunicationError::ChannelError(
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
                                if node_info.contains_wanted_block(&block_id) {
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
                let ops: Vec<Operation> = ops.into_values().collect();
                for (node, _) in self.active_nodes.iter() {
                    self.network_command_sender
                        .send_operations(node.clone(), ops.clone())
                        .await?
                }
            }
        }
        massa_trace!("protocol.protocol_worker.process_command.end", {});
        Ok(())
    }

    fn stop_asking_blocks(
        &mut self,
        remove_hashes: HashSet<BlockId>,
    ) -> Result<(), CommunicationError> {
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
    ) -> Result<(), CommunicationError> {
        massa_trace!("protocol.protocol_worker.update_ask_block.begin", {});

        let now = Instant::now();

        // init timer
        let mut next_tick = now
            .checked_add(self.cfg.ask_block_timeout.into())
            .ok_or(TimeError::TimeOverflowError)?;

        // list blocks to re-ask and gather candidate nodes to ask from
        let mut candidate_nodes: HashMap<BlockId, Vec<_>> = HashMap::new();
        let mut ask_block_list: HashMap<NodeId, Vec<BlockId>> = HashMap::new();

        // list blocks to re-ask and from whom
        for hash in self.block_wishlist.iter() {
            let mut needs_ask = true;

            for (node_id, node_info) in self.active_nodes.iter_mut() {
                //map to remove the borrow on asked_blocks. Otherwise can't call insert_known_block
                let ask_time_opt = node_info.asked_blocks.get(hash).map(|time| *time);
                let (timeout_at_opt, timed_out) = if let Some(ask_time) = ask_time_opt {
                    let t = ask_time
                        .checked_add(self.cfg.ask_block_timeout.into())
                        .ok_or(TimeError::TimeOverflowError)?;
                    (Some(t), t <= now)
                } else {
                    (None, false)
                };
                let knows_block = node_info.get_known_block(&hash);

                // check if the node recently told us it doesn't have the block
                if let Some((false, info_time)) = knows_block {
                    let info_expires = info_time
                        .checked_add(self.cfg.ask_block_timeout.into())
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
                        needs_ask = false; // no need to reask
                        continue; // not a candidate
                    }
                    // timed out, supposed to have it
                    (true, Some(timeout_at), Some((true, info_time))) => {
                        if info_time < &timeout_at {
                            // info less recent than timeout: mark as not having it
                            node_info.insert_known_block(
                                *hash,
                                false,
                                timeout_at,
                                self.cfg.max_node_known_blocks_size,
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
                            node_info.insert_known_block(
                                *hash,
                                false,
                                timeout_at,
                                self.cfg.max_node_known_blocks_size,
                            );
                        }
                        (2u8, ask_time_opt)
                    }
                    // timed out but don't know if has it: mark as not having it
                    (true, Some(timeout_at), None) => {
                        node_info.insert_known_block(
                            *hash,
                            false,
                            timeout_at,
                            self.cfg.max_node_known_blocks_size,
                        );
                        (2u8, ask_time_opt)
                    }
                };

                // add candidate node
                candidate_nodes
                    .entry(*hash)
                    .or_insert_with(|| Vec::new())
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
                                .checked_add(self.cfg.ask_block_timeout.into())
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
                        <= self.cfg.max_simultaneous_ask_blocks_per_node
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
                    .or_insert_with(|| vec![])
                    .push(hash);

                let timeout_at = now
                    .checked_add(self.cfg.ask_block_timeout.into())
                    .ok_or(TimeError::TimeOverflowError)?;
                next_tick = std::cmp::min(next_tick, timeout_at);
            }
        }

        //send AskBlockEvents
        if !ask_block_list.is_empty() {
            massa_trace!("protocol.protocol_worker.update_ask_block", {
                "list": ask_block_list
            });
            self.network_command_sender
                .ask_for_block_list(ask_block_list)
                .await
                .map_err(|_| {
                    CommunicationError::ChannelError(
                        "ask for block node command send failed".into(),
                    )
                })?;
        }

        // reset timer
        ask_block_timer.set(sleep_until(next_tick));

        Ok(())
    }

    // Ban a node.
    async fn ban_node(&mut self, node_id: &NodeId) -> Result<(), CommunicationError> {
        massa_trace!("protocol.protocol_worker.ban_node", { "node": node_id });
        self.active_nodes.remove(node_id);
        self.network_command_sender
            .ban(node_id.clone())
            .await
            .map_err(|_| CommunicationError::ChannelError("Ban node command send failed".into()))?;
        Ok(())
    }

    /// Check a header's signature, and if valid note the node knows the block.
    async fn note_header_from_node(
        &mut self,
        header: &BlockHeader,
        source_node_id: &NodeId,
    ) -> Result<Option<BlockId>, CommunicationError> {
        massa_trace!("protocol.protocol_worker.note_header_from_node", { "node": source_node_id, "header": header });
        match header.verify_integrity(&self.serialization_context) {
            Ok(block_id) => {
                if let Some(node_info) = self.active_nodes.get_mut(source_node_id) {
                    node_info.insert_known_block(
                        block_id,
                        true,
                        Instant::now(),
                        self.cfg.max_node_known_blocks_size,
                    );
                    massa_trace!("protocol.protocol_worker.note_header_from_node.ok", { "node": source_node_id,"block_id":block_id, "header": header});
                    return Ok(Some(block_id));
                }
                Ok(None)
            }
            Err(err) => {
                massa_trace!("protocol.protocol_worker.note_header_from_node.err", { "node": source_node_id, "header": header, "err": format!("{:?}", err)});
                Ok(None)
            }
        }
    }

    /// Check a header's signature, and if valid note the node knows the block.
    async fn note_block_from_node(
        &mut self,
        block: &Block,
        source_node_id: &NodeId,
    ) -> Result<Option<(BlockId, HashMap<OperationId, usize>)>, CommunicationError> {
        massa_trace!("protocol.protocol_worker.note_block_from_node", { "node": source_node_id, "block": block });

        // check header
        let block_id = match block.header.verify_integrity(&self.serialization_context) {
            Ok(block_id) => block_id,
            Err(err) => {
                massa_trace!("protocol.protocol_worker.note_block_from_node.err_header", { "node": source_node_id, "block": block, "err": format!("{:?}", err)});
                return Ok(None);
            }
        };

        // check operations (period, reuse, signatures, thread)
        let mut op_ids: Vec<OperationId> = Vec::with_capacity(block.operations.len());
        let mut seen_ops: HashMap<OperationId, usize> =
            HashMap::with_capacity(block.operations.len());
        for (idx, op) in block.operations.iter().enumerate() {
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
            match Address::from_public_key(&op.content.sender_public_key) {
                Ok(addr) => {
                    if addr.get_thread(self.serialization_context.parent_count)
                        != block.header.content.slot.thread
                    {
                        massa_trace!("protocol.protocol_worker.note_block_from_node.err_op_thread",
                            { "node": source_node_id,"block_id":block_id, "block": block, "op": op});
                        return Ok(None);
                    }
                }
                Err(err) => {
                    massa_trace!("protocol.protocol_worker.note_block_from_node.err_op_creator_address",
                        { "node": source_node_id,"block_id":block_id, "block": block, "op": op, "err": format!("{:?}", err)});
                    return Ok(None);
                }
            }

            // check integrity (signature) and reuse
            match op.verify_integrity(&self.serialization_context) {
                Ok(op_id) => {
                    if seen_ops.insert(op_id, idx).is_some() {
                        // reused
                        massa_trace!("protocol.protocol_worker.note_block_from_node.err_op_reused",
                            { "node": source_node_id,"block_id":block_id, "block": block, "op": op });
                        return Ok(None);
                    }
                    op_ids.push(op_id);
                }
                Err(err) => {
                    massa_trace!("protocol.protocol_worker.note_block_from_node.err_op_integrity",
                        { "node": source_node_id,"block_id":block_id, "block": block, "op": op, "err": format!("{:?}", err)});
                    return Ok(None);
                }
            }
        }

        // check root hash
        {
            let concat_bytes = op_ids
                .iter()
                .map(|op_id| op_id.to_bytes().to_vec())
                .concat();
            if block.header.content.operation_merkle_root != Hash::hash(&concat_bytes) {
                massa_trace!("protocol.protocol_worker.note_block_from_node.err_op_root_hash",
                    { "node": source_node_id,"block_id":block_id, "block": block });
                return Ok(None);
            }
        }

        // add to known blocks
        if let Some(node_info) = self.active_nodes.get_mut(source_node_id) {
            node_info.insert_known_block(
                block_id,
                true,
                Instant::now(),
                self.cfg.max_node_known_blocks_size,
            );
            massa_trace!("protocol.protocol_worker.note_block_from_node.ok", { "node": source_node_id,"block_id":block_id, "block": block});
            return Ok(Some((block_id, seen_ops)));
        }
        Ok(None)
    }

    /// Check operations
    fn note_operations_from_node(
        &mut self,
        operations: Vec<Operation>,
        source_node_id: &NodeId,
    ) -> Option<HashMap<OperationId, Operation>> {
        massa_trace!("protocol.protocol_worker.note_operations_from_node", { "node": source_node_id, "opearations": operations });
        let mut result = HashMap::new();
        for op in operations.into_iter() {
            match op.verify_integrity(&self.serialization_context) {
                Ok(operation_id) => {
                    result.insert(operation_id, op);
                }
                Err(err) => {
                    massa_trace!("protocol.protocol_worker.note_operations_from_node.invalid", { "node": source_node_id, "opearation": op, "err": format!("{:?}", err) });
                    warn!(
                        "node {:?} sent an invalid operation: {:?}",
                        source_node_id, err
                    );
                    return None;
                }
            }
        }
        Some(result)
    }

    /// Manages network event
    /// Only used by the worker.
    ///
    /// # Argument
    /// evt: event to processs
    async fn on_network_event(
        &mut self,
        evt: NetworkEvent,
        block_ask_timer: &mut std::pin::Pin<&mut Sleep>,
    ) -> Result<(), CommunicationError> {
        match evt {
            NetworkEvent::NewConnection(node_id) => {
                massa_trace!(
                    "protocol.protocol_worker.on_network_event.new_connection",
                    { "node": node_id }
                );
                self.active_nodes.insert(node_id, nodeinfo::NodeInfo::new());
                self.update_ask_block(block_ask_timer).await?;
            }
            NetworkEvent::ConnectionClosed(node_id) => {
                massa_trace!(
                    "protocol.protocol_worker.on_network_event.connection_closed",
                    { "node": node_id }
                );
                self.active_nodes.remove(&node_id); // deletes all node info
                self.update_ask_block(block_ask_timer).await?;
            }
            NetworkEvent::ReceivedBlock {
                node: from_node_id,
                block,
            } => {
                massa_trace!("protocol.protocol_worker.on_network_event.received_block", { "node": from_node_id, "block": block});
                if let Some((block_id, operation_set)) =
                    self.note_block_from_node(&block, &from_node_id).await?
                {
                    let mut set = HashSet::with_capacity(1);
                    set.insert(block_id);
                    self.stop_asking_blocks(set)?;
                    self.send_protocol_event(ProtocolEvent::ReceivedBlock {
                        block_id,
                        block,
                        operation_set,
                    })
                    .await;
                    self.update_ask_block(block_ask_timer).await?;
                } else {
                    warn!("node {:?} sent us critically incorrect block", from_node_id);
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
                        node_info.insert_wanted_blocks(
                            hash.clone(),
                            self.cfg.max_node_wanted_blocks_size,
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
                if let Some(block_id) = self.note_header_from_node(&header, &source_node_id).await?
                {
                    self.send_protocol_event(ProtocolEvent::ReceivedBlockHeader {
                        block_id,
                        header,
                    })
                    .await;
                    self.update_ask_block(block_ask_timer).await?;
                } else {
                    warn!(
                        "node {:?} sent us critically incorrect header",
                        source_node_id,
                    );
                    let _ = self.ban_node(&source_node_id).await;
                }
            }
            NetworkEvent::BlockNotFound { node, block_id } => {
                massa_trace!("protocol.protocol_worker.on_network_event.block_not_found", { "node": node, "block_id": block_id});
                if let Some(info) = self.active_nodes.get_mut(&node) {
                    info.insert_known_block(
                        block_id,
                        false,
                        Instant::now(),
                        self.cfg.max_node_known_blocks_size,
                    );
                }
                self.update_ask_block(block_ask_timer).await?;
            }
            NetworkEvent::ReceivedOperations { node, operations } => {
                massa_trace!("protocol.protocol_worker.on_network_event.received_operations", { "node": node, "operations": operations});
                if let Some(operations) = self.note_operations_from_node(operations.clone(), &node)
                {
                    if !operations.is_empty() {
                        self.send_protocol_pool_event(ProtocolPoolEvent::ReceivedOperations(
                            operations,
                        ))
                        .await;
                    }
                } else {
                    warn!("node {:?} sent us critically incorrect operation", node,);
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
    use serial_test::serial;

    #[test]
    #[serial]
    fn test_node_info_know_block() {
        let max_node_known_blocks_size = 10;
        let mut nodeinfo = NodeInfo::new();
        let instant = Instant::now();

        let hash_test = BlockId::for_tests("test").unwrap();
        nodeinfo.insert_known_block(hash_test, true, instant, max_node_known_blocks_size);
        let (val, t) = nodeinfo.get_known_block(&hash_test).unwrap();
        assert!(val);
        assert_eq!(instant, *t);
        nodeinfo.insert_known_block(hash_test, false, instant, max_node_known_blocks_size);
        let (val, t) = nodeinfo.get_known_block(&hash_test).unwrap();
        assert!(!val);
        assert_eq!(instant, *t);

        for index in 0..9 {
            let hash = BlockId::for_tests(&index.to_string()).unwrap();
            nodeinfo.insert_known_block(hash, true, Instant::now(), max_node_known_blocks_size);
            assert!(nodeinfo.get_known_block(&hash).is_some());
        }

        //re insert the oldest to update its timestamp.
        nodeinfo.insert_known_block(hash_test, false, Instant::now(), max_node_known_blocks_size);

        //add hash that triggers container pruning
        nodeinfo.insert_known_block(
            BlockId::for_tests("test2").unwrap(),
            true,
            Instant::now(),
            max_node_known_blocks_size,
        );

        //test should be present
        assert!(nodeinfo
            .get_known_block(&BlockId::for_tests("test").unwrap())
            .is_some());
        //0 should be remove because it's the oldest.
        assert!(nodeinfo
            .get_known_block(&BlockId::for_tests(&0.to_string()).unwrap())
            .is_none());
        //the other are still present.
        for index in 1..9 {
            let hash = BlockId::for_tests(&index.to_string()).unwrap();
            assert!(nodeinfo.get_known_block(&hash).is_some());
        }
    }

    #[test]
    #[serial]
    fn test_node_info_wanted_block() {
        let max_node_wanted_blocks_size = 10;
        let mut nodeinfo = NodeInfo::new();

        let hash = BlockId::for_tests("test").unwrap();
        nodeinfo.insert_wanted_blocks(hash, max_node_wanted_blocks_size);
        assert!(nodeinfo.contains_wanted_block(&hash));
        nodeinfo.remove_wanted_block(&hash);
        assert!(!nodeinfo.contains_wanted_block(&hash));

        for index in 0..9 {
            let hash = BlockId::for_tests(&index.to_string()).unwrap();
            nodeinfo.insert_wanted_blocks(hash, max_node_wanted_blocks_size);
            assert!(nodeinfo.contains_wanted_block(&hash));
        }

        // change the oldest time to now
        assert!(nodeinfo.contains_wanted_block(&BlockId::for_tests(&0.to_string()).unwrap()));
        //add hash that triggers container pruning
        nodeinfo.insert_wanted_blocks(
            BlockId::for_tests("test2").unwrap(),
            max_node_wanted_blocks_size,
        );

        //0 is present because because its timestamp has been updated with contains_wanted_block
        assert!(nodeinfo.contains_wanted_block(&BlockId::for_tests(&0.to_string()).unwrap()));

        //1 has been removed because it's the oldest.
        assert!(nodeinfo.contains_wanted_block(&BlockId::for_tests(&1.to_string()).unwrap()));

        //Other blocks are present.
        for index in 2..9 {
            let hash = BlockId::for_tests(&index.to_string()).unwrap();
            assert!(nodeinfo.contains_wanted_block(&hash));
        }
    }
}
