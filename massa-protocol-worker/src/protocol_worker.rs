//! Copyright (c) 2022 MASSA LABS <info@massa.net>

use crate::checked_operations::CheckedOperations;
use crate::{node_info::NodeInfo, worker_operations_impl::OperationBatchBuffer};

use massa_logging::massa_trace;

use massa_models::{
    constants::CHANNEL_SIZE,
    node::NodeId,
    operation::OperationPrefixId,
    prehash::{BuildMap, Map, Set},
    BlockHeaderSerializer, BlockId, EndorsementId, OperationId, WrappedEndorsement, WrappedHeader,
};
use massa_models::{EndorsementSerializer, OperationSerializer, WrappedOperation};
use massa_network_exports::{AskForBlocksInfo, NetworkCommandSender, NetworkEventReceiver};
use massa_protocol_exports::{
    ProtocolCommand, ProtocolCommandSender, ProtocolConfig, ProtocolError, ProtocolEvent,
    ProtocolEventReceiver, ProtocolManagementCommand, ProtocolManager, ProtocolPoolEvent,
    ProtocolPoolEventReceiver,
};

use massa_storage::Storage;
use massa_time::TimeError;
use std::collections::{HashMap, HashSet};
use tokio::{
    sync::mpsc,
    sync::mpsc::error::SendTimeoutError,
    sync::oneshot,
    time::{sleep, sleep_until, Instant, Sleep},
};
use tracing::{debug, error, info, warn};

// TODO connect protocol to pool so that it sends ops and endorsements

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
    storage: Storage,
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
            config,
            ProtocolWorkerChannels {
                network_command_sender,
                network_event_receiver,
                controller_event_tx,
                controller_pool_event_tx,
                controller_command_rx,
                controller_manager_rx,
            },
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
        ProtocolPoolEventReceiver(pool_event_rx),
        ProtocolManager::new(join_handle, manager_tx),
    ))
}

/// Info about a block we've seen
pub(crate) struct BlockInfo {
    /// Endorsements contained in the block header.
    pub(crate) endorsements: Map<EndorsementId, u32>,
    /// Operations contained in the block,
    /// if we've received them already, and none otherwise.
    pub(crate) operations: Option<Vec<OperationId>>,
    /// The header of the block.
    pub(crate) header: WrappedHeader,
    /// Full operations size in bytes
    pub(crate) operations_size: usize,
}

impl BlockInfo {
    fn new(endorsements: Map<EndorsementId, u32>, header: WrappedHeader) -> Self {
        BlockInfo {
            endorsements,
            operations: None,
            header,
            operations_size: 0,
        }
    }
}

/// protocol worker
pub struct ProtocolWorker {
    /// Protocol configuration.
    pub(crate) config: ProtocolConfig,
    /// Associated network command sender.
    pub(crate) network_command_sender: NetworkCommandSender,
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
    pub(crate) active_nodes: HashMap<NodeId, NodeInfo>,
    /// List of wanted blocks,
    /// with the info representing their state withint the as_block workflow.
    pub(crate) block_wishlist: Map<BlockId, AskForBlocksInfo>,
    /// List of processed endorsements
    checked_endorsements: Set<EndorsementId>,
    /// List of processed operations
    pub(crate) checked_operations: CheckedOperations,
    /// List of processed headers
    pub(crate) checked_headers: Map<BlockId, BlockInfo>,
    /// List of ids of operations that we asked to the nodes
    pub(crate) asked_operations: HashMap<OperationPrefixId, (Instant, Vec<NodeId>)>,
    /// Buffer for operations that we want later
    pub(crate) op_batch_buffer: OperationBatchBuffer,
    /// Shared storage.
    pub(crate) storage: Storage,
}

/// channels used by the protocol worker
pub struct ProtocolWorkerChannels {
    /// network command sender
    pub network_command_sender: NetworkCommandSender,
    /// network event receiver
    pub network_event_receiver: NetworkEventReceiver,
    /// protocol event sender
    pub controller_event_tx: mpsc::Sender<ProtocolEvent>,
    /// protocol pool event sender
    pub controller_pool_event_tx: mpsc::Sender<ProtocolPoolEvent>,
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
            controller_pool_event_tx,
            controller_command_rx,
            controller_manager_rx,
        }: ProtocolWorkerChannels,
        storage: Storage,
    ) -> ProtocolWorker {
        ProtocolWorker {
            config,
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
            asked_operations: Default::default(),
            op_batch_buffer: OperationBatchBuffer::with_capacity(
                config.operation_batch_buffer_capacity,
            ),
            storage,
        }
    }

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

    pub(crate) async fn send_protocol_pool_event(&self, event: ProtocolPoolEvent) {
        let result = self
            .controller_pool_event_tx
            .send_timeout(event, self.config.max_send_wait.to_duration())
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

    async fn process_command(
        &mut self,
        cmd: ProtocolCommand,
        timer: &mut std::pin::Pin<&mut Sleep>,
    ) -> Result<(), ProtocolError> {
        match cmd {
            ProtocolCommand::IntegratedBlock {
                block_id,
                storage: _,
            } => {
                // TODO properly manage storage
                massa_trace!(
                    "protocol.protocol_worker.process_command.integrated_block.begin",
                    { "block_id": block_id }
                );
                for (node_id, node_info) in self.active_nodes.iter_mut() {
                    // node that isn't asking for that block
                    let cond = node_info.get_known_block(&block_id);
                    // if we don't know if that node knows that hash or if we know it doesn't
                    if !cond.map_or_else(|| false, |v| v.0) {
                        massa_trace!("protocol.protocol_worker.process_command.integrated_block.send_header", { "node": node_id, "block_id": block_id});
                        self.network_command_sender
                            .send_block_header(*node_id, block_id)
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
                self.stop_asking_blocks(remove)?;
                for block in new.into_iter() {
                    self.block_wishlist.insert(block, AskForBlocksInfo::Info);
                }
                self.update_ask_block(timer).await?;
                massa_trace!(
                    "protocol.protocol_worker.process_command.wishlist_delta.end",
                    {}
                );
            }
            ProtocolCommand::PropagateOperations(operation_ids) => {
                massa_trace!(
                    "protocol.protocol_worker.process_command.propagate_operations.begin",
                    { "operation_ids": operation_ids }
                );
                for id in operation_ids.iter() {
                    self.checked_operations.insert(id);
                }
                for (node, node_info) in self.active_nodes.iter_mut() {
                    let new_ops: Set<OperationId> = operation_ids
                        .iter()
                        .filter(|id| !node_info.knows_op(id))
                        .copied()
                        .collect();
                    node_info.insert_known_ops(
                        new_ops.iter().cloned().collect(),
                        self.config.max_node_known_ops_size,
                    );
                    if !new_ops.is_empty() {
                        self.network_command_sender
                            .send_operations_batch(
                                *node,
                                new_ops.iter().map(|id| id.into_prefix()).collect(),
                            )
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
                    let new_endorsements: Map<EndorsementId, WrappedEndorsement> = endorsements
                        .iter()
                        .filter(|(id, _)| !node_info.knows_endorsement(id))
                        .map(|(k, v)| (*k, v.clone()))
                        .collect();
                    node_info.insert_known_endorsements(
                        new_endorsements.keys().copied().collect(),
                        self.config.max_node_known_endorsements_size,
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

    /// Remove the given blocks from the local wishlist
    pub(crate) fn stop_asking_blocks(
        &mut self,
        remove_hashes: Set<BlockId>,
    ) -> Result<(), ProtocolError> {
        massa_trace!("protocol.protocol_worker.stop_asking_blocks", {
            "remove": remove_hashes
        });
        for node_info in self.active_nodes.values_mut() {
            node_info
                .asked_blocks
                .retain(|h, _| !remove_hashes.contains(h));
        }
        self.block_wishlist
            .retain(|h, _| !remove_hashes.contains(h));
        Ok(())
    }

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

        // list blocks to re-ask and gather candidate nodes to ask from
        let mut candidate_nodes: Map<BlockId, Vec<_>> = Default::default();
        let mut ask_block_list: HashMap<NodeId, Vec<(BlockId, AskForBlocksInfo)>> =
            Default::default();

        // list blocks to re-ask and from whom
        for (hash, required_info) in self.block_wishlist.iter() {
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
                    required_info,
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
        if let Err(err) =
            header.verify_signature(BlockHeaderSerializer::new(), &header.creator_public_key)
        {
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

        if self
            .checked_headers
            .insert(
                block_id,
                BlockInfo::new(endorsement_ids.clone(), header.clone()),
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
                self.config.max_node_known_blocks_size,
            );
            node_info.insert_known_blocks(
                &[block_id],
                true,
                now,
                self.config.max_node_known_blocks_size,
            );
            massa_trace!("protocol.protocol_worker.note_header_from_node.ok", { "node": source_node_id,"block_id":block_id, "header": header});
            return Ok(Some((block_id, endorsement_ids, true)));
        }
        Ok(None)
    }

    /// Prune `checked_endorsements` if it is too large
    fn prune_checked_endorsements(&mut self) {
        if self.checked_endorsements.len() > self.config.max_known_endorsements_size {
            self.checked_endorsements.clear();
        }
    }

    /// Prune `checked_operations` if it has grown too large.
    fn prune_checked_operations(&mut self) {
        if self.checked_operations.len() > self.config.max_known_ops_size {
            self.checked_operations.clear();
        }
    }

    /// Prune `checked_headers` if it is too large
    fn prune_checked_headers(&mut self) {
        if self.checked_headers.len() > self.config.max_known_blocks_size {
            self.checked_headers.clear();
        }
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
    pub(crate) async fn note_operations_from_node(
        &mut self,
        operations: Vec<WrappedOperation>,
        source_node_id: &NodeId,
        done_signal: Option<oneshot::Sender<()>>,
    ) -> Result<(Vec<OperationId>, Map<OperationId, usize>), ProtocolError> {
        massa_trace!("protocol.protocol_worker.note_operations_from_node", { "node": source_node_id, "operations": operations });
        let length = operations.len();
        let mut seen_ops = vec![];
        let mut new_operations = Map::with_capacity_and_hasher(length, BuildMap::default());
        let mut received_ids = Map::with_capacity_and_hasher(length, BuildMap::default());
        for (idx, operation) in operations.into_iter().enumerate() {
            let operation_id = operation.id;
            seen_ops.push(operation_id);
            received_ids.insert(operation_id, idx);
            // Check operation signature only if not already checked.
            if self.checked_operations.insert(&operation_id) {
                // check signature
                operation
                    .verify_signature(OperationSerializer::new(), &operation.creator_public_key)?;

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
            // Add to pool, propagate when received outside of a header.
            self.send_protocol_pool_event(ProtocolPoolEvent::ReceivedOperations {
                operations: new_operations,
                done_signal,
            })
            .await;

            // prune checked operations cache
            self.prune_checked_operations();
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
    pub(crate) async fn note_endorsements_from_node(
        &mut self,
        endorsements: Vec<WrappedEndorsement>,
        source_node_id: &NodeId,
        propagate: bool,
    ) -> Result<(Map<EndorsementId, u32>, bool), ProtocolError> {
        massa_trace!("protocol.protocol_worker.note_endorsements_from_node", { "node": source_node_id, "endorsements": endorsements});
        let length = endorsements.len();
        let mut contains_duplicates = false;

        let mut new_endorsements = Map::with_capacity_and_hasher(length, BuildMap::default());
        let mut endorsement_ids = Map::default();
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
                endorsement.verify_signature(
                    EndorsementSerializer::new(),
                    &endorsement.creator_public_key,
                )?;
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
