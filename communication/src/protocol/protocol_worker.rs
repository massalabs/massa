use super::config::ProtocolConfig;
use crate::common::NodeId;
use crate::error::CommunicationError;
use crate::network::{NetworkCommandSender, NetworkEvent, NetworkEventReceiver};
use crypto::hash::Hash;
use crypto::signature::SignatureEngine;
use models::{Block, BlockHeader, SerializationContext};
use std::collections::{HashMap, HashSet};
use time::TimeError;
use tokio::{
    sync::mpsc,
    time::{sleep, sleep_until, Instant, Sleep},
};

/// Possible types of events that can happen.
#[derive(Debug)]
pub enum ProtocolEvent {
    /// A isolated transaction was received.
    ReceivedTransaction(String),
    /// A block with a valid signature has been received.
    ReceivedBlock { hash: Hash, block: Block },
    /// A block header with a valid signature has been received.
    ReceivedBlockHeader { hash: Hash, header: BlockHeader },
    /// Ask for a block from consensus.
    GetBlock(Hash),
}

/// Commands that protocol worker can process
#[derive(Debug)]
pub enum ProtocolCommand {
    /// Notify block integration of a given block.
    IntegratedBlock {
        hash: Hash,
        block: Block,
    },
    /// A block, or it's header, amounted to an attempted attack.
    AttackBlockDetected(Hash),
    /// Send a block to peers who asked for it.
    FoundBlock {
        hash: Hash,
        block: Block,
    },
    /// Wishlist delta
    WishlistDelta {
        new: HashSet<Hash>,
        remove: HashSet<Hash>,
    },
    BlockNotFound(Hash),
}

#[derive(Debug)]
pub enum ProtocolManagementCommand {}

//put in a module to block private access from Protocol_worker.
mod nodeinfo {
    use crypto::hash::Hash;
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
        known_blocks: HashMap<Hash, (bool, Instant)>,
        /// The blocks the node asked for.
        wanted_blocks: HashMap<Hash, Instant>,
        /// Blocks we asked that node for
        pub asked_blocks: HashMap<Hash, Instant>,
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

        pub fn get_known_block(&self, hash: &Hash) -> Option<&(bool, Instant)> {
            self.known_blocks.get(hash)
        }

        pub fn insert_known_block(
            &mut self,
            hash: Hash,
            val: bool,
            instant: Instant,
            max_node_known_blocks_size: usize,
        ) {
            self.known_blocks.insert(hash, (val, instant));
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

        pub fn insert_wanted_blocks(&mut self, hash: Hash, max_node_wanted_blocks_size: usize) {
            self.wanted_blocks.insert(hash, Instant::now());
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

        pub fn contains_wanted_block(&mut self, hash: &Hash) -> bool {
            self.wanted_blocks
                .get_mut(hash)
                .map(|instant| *instant = Instant::now())
                .is_some()
        }

        pub fn remove_wanted_block(&mut self, hash: &Hash) -> bool {
            self.wanted_blocks.remove(hash).is_some()
        }
    }
}

pub struct ProtocolWorker {
    /// Protocol configuration.
    cfg: ProtocolConfig,
    // Serialization context
    serialization_context: SerializationContext,
    /// Associated nework command sender.
    network_command_sender: NetworkCommandSender,
    /// Associated nework event receiver.
    network_event_receiver: NetworkEventReceiver,
    /// Channel to send protocol events to the controller.
    controller_event_tx: mpsc::Sender<ProtocolEvent>,
    /// Channel receiving commands from the controller.
    controller_command_rx: mpsc::Receiver<ProtocolCommand>,
    /// Channel to send management commands to the controller.
    controller_manager_rx: mpsc::Receiver<ProtocolManagementCommand>,
    /// Ids of active nodes mapped to node info.
    active_nodes: HashMap<NodeId, nodeinfo::NodeInfo>,
    /// List of wanted blocks.
    block_wishlist: HashSet<Hash>,
}

impl ProtocolWorker {
    /// Creates a new protocol worker.
    ///
    /// # Arguments
    /// * cfg: protocol configuration.
    /// * self_node_id: our private key.
    /// * network_controller associated network controller.
    /// * controller_event_tx: Channel to send protocol events.
    /// * controller_command_rx: Channel receiving commands.
    /// * controller_manager_rx: Channel receiving management commands.
    pub fn new(
        cfg: ProtocolConfig,
        serialization_context: SerializationContext,
        network_command_sender: NetworkCommandSender,
        network_event_receiver: NetworkEventReceiver,
        controller_event_tx: mpsc::Sender<ProtocolEvent>,
        controller_command_rx: mpsc::Receiver<ProtocolCommand>,
        controller_manager_rx: mpsc::Receiver<ProtocolManagementCommand>,
    ) -> ProtocolWorker {
        ProtocolWorker {
            serialization_context,
            cfg,
            network_command_sender,
            network_event_receiver,
            controller_event_tx,
            controller_command_rx,
            controller_manager_rx,
            active_nodes: HashMap::new(),
            block_wishlist: HashSet::new(),
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
            trace!("before select! in protocol_worker run_loop");
            tokio::select! {
                // block ask timer
                _ = &mut block_ask_timer => {
                    trace!("after select! block_ask_timer in protocol_worker run_loop");
                    self.update_ask_block(&mut block_ask_timer).await?;
                }

                // listen to incoming commands
                Some(cmd) = self.controller_command_rx.recv() => {
                    trace!("after select! Some(cmd) in protocol_worker run_loop cmd:{:?}", cmd);
                    self.process_command(cmd, &mut block_ask_timer).await?;
                }

                // listen to network controller events
                evt = self.network_event_receiver.wait_event() => {
                    trace!("after select! evt in protocol_worker run_loop");
                    self.on_network_event(evt?, &mut block_ask_timer).await?;
                }

                // listen to management commands
                cmd = self.controller_manager_rx.recv() => {
                    trace!("after select! cmd in protocol_worker run_loop");
                    match cmd {
                        None => break,
                        Some(_) => {}
                    };
                }
            } //end select!
        } //end loop

        Ok(self.network_event_receiver)
    }

    async fn process_command(
        &mut self,
        cmd: ProtocolCommand,
        timer: &mut std::pin::Pin<&mut Sleep>,
    ) -> Result<(), CommunicationError> {
        match cmd {
            ProtocolCommand::IntegratedBlock { hash, block } => {
                massa_trace!("protocol_worker process_command integrated_block", { "block_header": block.header });
                for (node_id, node_info) in self.active_nodes.iter_mut() {
                    // if we know that a node wants a block we send the full block
                    if node_info.remove_wanted_block(&hash) {
                        node_info.insert_known_block(
                            hash,
                            true,
                            Instant::now(),
                            self.cfg.max_node_known_blocks_size,
                        );
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
                        let cond = node_info.get_known_block(&hash);
                        // if we don't know if that node knowns that hash or if we know it doesn't
                        if !cond.map_or_else(|| false, |v| v.0) {
                            self.network_command_sender
                                .send_block_header(*node_id, block.header.clone())
                                .await
                                .map_err(|_| {
                                    CommunicationError::ChannelError(
                                        "send block header network command send failed".into(),
                                    )
                                })?;
                        } else {
                            // todo broadcast hash (see #202)
                        }
                    }
                }
            }
            ProtocolCommand::AttackBlockDetected(hash) => {
                // Ban all the nodes that sent us this object.
                debug!("protocol_worker process_command ProtocolCommand::AttackBlockDetected");
                let to_ban: Vec<NodeId> = self
                    .active_nodes
                    .iter()
                    .filter_map(|(id, info)| match info.get_known_block(&hash) {
                        Some((true, _)) => Some(id.clone()),
                        _ => None,
                    })
                    .collect();
                for id in to_ban.iter() {
                    self.ban_node(id).await?;
                }
            }
            ProtocolCommand::FoundBlock { hash, block } => {
                debug!("protocol_worker process_command ProtocolCommand::FoundBlock");
                // Send the block once to all nodes who asked for it.
                for (node_id, node_info) in self.active_nodes.iter_mut() {
                    if node_info.remove_wanted_block(&hash) {
                        node_info.insert_known_block(
                            hash,
                            true,
                            Instant::now(),
                            self.cfg.max_node_known_blocks_size,
                        );
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
            }
            ProtocolCommand::WishlistDelta { new, remove } => {
                debug!("protocol_worker process_command ProtocolCommand::WishlistDelta");
                self.stop_asking_blocks(remove)?;
                self.block_wishlist.extend(new);
                self.update_ask_block(timer).await?;
            }
            ProtocolCommand::BlockNotFound(hash) => {
                debug!("protocol_worker process_command ProtocolCommand::BlockNotFound");
                for (node_id, node_info) in self.active_nodes.iter_mut() {
                    if node_info.contains_wanted_block(&hash) {
                        self.network_command_sender
                            .block_not_found(*node_id, hash)
                            .await?
                    }
                }
            }
        }
        debug!("END protocol_worker");
        Ok(())
    }

    fn stop_asking_blocks(
        &mut self,
        remove_hashes: HashSet<Hash>,
    ) -> Result<(), CommunicationError> {
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
        let now = Instant::now();

        // init timer
        let mut next_tick = now
            .checked_add(self.cfg.ask_block_timeout.into())
            .ok_or(TimeError::TimeOverflowError)?;

        let mut ask_block_list: HashMap<NodeId, HashSet<Hash>> = HashMap::new();

        // list blocks to re-ask and from whom
        for hash in self.block_wishlist.iter() {
            let mut needs_ask = true;
            let mut best_candidate = None;

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

                // update candidate node
                if best_candidate.is_none()
                    || Some((candidate, node_info.connection_instant, *node_id)) < best_candidate
                {
                    best_candidate = Some((candidate, node_info.connection_instant, *node_id));
                }
            }

            // skip if doesn't need to be asked
            if !needs_ask {
                continue;
            }

            // ask the best node, if there is one and update timeout
            if let Some((_, _, node_id)) = best_candidate.take() {
                self.active_nodes
                    .get_mut(&node_id)
                    .unwrap()
                    .asked_blocks
                    .insert(*hash, now); // will not panic, already checked

                let hash_list = ask_block_list.entry(node_id).or_insert(HashSet::new());
                hash_list.insert(*hash);

                let timeout_at = now
                    .checked_add(self.cfg.ask_block_timeout.into())
                    .ok_or(TimeError::TimeOverflowError)?;
                next_tick = std::cmp::min(next_tick, timeout_at);
            }
        }

        //send AskBlockEvents
        if !ask_block_list.is_empty() {
            self.network_command_sender
                .ask_for_block(ask_block_list)
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
    ) -> Result<Option<Hash>, CommunicationError> {
        if let Ok(hash) = header.content.compute_hash(&self.serialization_context) {
            // check signature
            if let Err(_err) = header.verify_signature(&hash, &SignatureEngine::new()) {
                return self.ban_node(source_node_id).await.map(|_| None);
            }

            if let Some(node_info) = self.active_nodes.get_mut(source_node_id) {
                node_info.insert_known_block(
                    hash,
                    true,
                    Instant::now(),
                    self.cfg.max_node_known_blocks_size,
                );
                return Ok(Some(hash));
            }
        }
        Ok(None)
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
                self.active_nodes.insert(node_id, nodeinfo::NodeInfo::new());
                self.update_ask_block(block_ask_timer).await?;
            }
            NetworkEvent::ConnectionClosed(node_id) => {
                self.active_nodes.remove(&node_id); // deletes all node info
                self.update_ask_block(block_ask_timer).await?;
            }
            NetworkEvent::ReceivedBlock {
                node: from_node_id,
                block,
            } => {
                if let Some(hash) = self
                    .note_header_from_node(&block.header, &from_node_id)
                    .await?
                {
                    self.stop_asking_blocks(HashSet::from(vec![hash].into_iter().collect()))?;
                    trace!("before sending  ProtocolEvent::ReceivedBlock from controller_event_tx in protocol_worker on_network_event");
                    self.controller_event_tx
                        .send(ProtocolEvent::ReceivedBlock { hash, block })
                        .await
                        .map_err(|_| {
                            CommunicationError::ChannelError(
                                "receive block event send failed".into(),
                            )
                        })?;
                    trace!("after sending  ProtocolEvent::ReceivedBlock from controller_event_tx in protocol_worker on_network_event");
                    self.update_ask_block(block_ask_timer).await?;
                } else {
                    warn!(
                        "node {:?} sent us critically incorrect block {:?}",
                        from_node_id, block
                    )
                }
            }
            NetworkEvent::AskedForBlocks {
                node: from_node_id,
                list,
            } => {
                if let Some(node_info) = self.active_nodes.get_mut(&from_node_id) {
                    for hash in list {
                        node_info.insert_wanted_blocks(hash, self.cfg.max_node_wanted_blocks_size);
                        trace!("before sending  ProtocolEvent::GetBlock from controller_event_tx in protocol_worker on_network_event");
                        self.controller_event_tx
                            .send(ProtocolEvent::GetBlock(hash))
                            .await
                            .map_err(|_| {
                                CommunicationError::ChannelError(
                                    "receive asked for block event send failed".into(),
                                )
                            })?;
                        trace!("after sending  ProtocolEvent::GetBlock from controller_event_tx in protocol_worker on_network_event");
                    }
                }
            }
            NetworkEvent::ReceivedBlockHeader {
                source_node_id,
                header,
            } => {
                if let Some(hash) = self.note_header_from_node(&header, &source_node_id).await? {
                    trace!("before sending  ProtocolEvent::ReceivedBlockHeader from controller_event_tx in protocol_worker on_network_event");
                    self.controller_event_tx
                        .send(ProtocolEvent::ReceivedBlockHeader { hash, header })
                        .await
                        .map_err(|_| {
                            CommunicationError::ChannelError(
                                "receive block event send failed".into(),
                            )
                        })?;
                    trace!("after sending  ProtocolEvent::ReceivedBlockHeader from controller_event_tx in protocol_worker on_network_event");
                    self.update_ask_block(block_ask_timer).await?;
                } else {
                    warn!(
                        "node {:?} sent us critically incorrect header {:?}",
                        source_node_id, header
                    )
                }
            }
            NetworkEvent::BlockNotFound { node, hash } => {
                if let Some(info) = self.active_nodes.get_mut(&node) {
                    info.insert_known_block(
                        hash,
                        false,
                        Instant::now(),
                        self.cfg.max_node_known_blocks_size,
                    );
                }
                self.update_ask_block(block_ask_timer).await?;
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::nodeinfo::NodeInfo;
    use super::*;

    #[test]
    fn test_node_info_know_block() {
        let max_node_known_blocks_size = 10;
        let mut nodeinfo = NodeInfo::new();
        let instant = Instant::now();

        let hash_test = Hash::hash("test".as_bytes());
        nodeinfo.insert_known_block(hash_test, true, instant, max_node_known_blocks_size);
        let (val, t) = nodeinfo.get_known_block(&hash_test).unwrap();
        assert!(val);
        assert_eq!(instant, *t);
        nodeinfo.insert_known_block(hash_test, false, instant, max_node_known_blocks_size);
        let (val, t) = nodeinfo.get_known_block(&hash_test).unwrap();
        assert!(!val);
        assert_eq!(instant, *t);

        for index in 0..9 {
            let hash = Hash::hash(index.to_string().as_bytes());
            println!("index:{} hash:{:?}", index, hash);
            nodeinfo.insert_known_block(hash, true, Instant::now(), max_node_known_blocks_size);
            assert!(nodeinfo.get_known_block(&hash).is_some());
        }

        //re insert the oldest to update its timestamp.
        nodeinfo.insert_known_block(hash_test, false, Instant::now(), max_node_known_blocks_size);

        //add hash that triggers container pruning
        nodeinfo.insert_known_block(
            Hash::hash("test2".as_bytes()),
            true,
            Instant::now(),
            max_node_known_blocks_size,
        );

        //test should be present
        assert!(nodeinfo
            .get_known_block(&Hash::hash("test".as_bytes()))
            .is_some());
        //0 should be remove because it's the oldest.
        assert!(nodeinfo
            .get_known_block(&Hash::hash(0.to_string().as_bytes()))
            .is_none());
        //the other are still present.
        for index in 1..9 {
            let hash = Hash::hash(index.to_string().as_bytes());
            assert!(nodeinfo.get_known_block(&hash).is_some());
        }
    }

    #[test]
    fn test_node_info_wanted_block() {
        let max_node_wanted_blocks_size = 10;
        let mut nodeinfo = NodeInfo::new();

        let hash = Hash::hash("test".as_bytes());
        nodeinfo.insert_wanted_blocks(hash, max_node_wanted_blocks_size);
        assert!(nodeinfo.contains_wanted_block(&hash));
        nodeinfo.remove_wanted_block(&hash);
        assert!(!nodeinfo.contains_wanted_block(&hash));

        for index in 0..9 {
            let hash = Hash::hash(index.to_string().as_bytes());
            nodeinfo.insert_wanted_blocks(hash, max_node_wanted_blocks_size);
            assert!(nodeinfo.contains_wanted_block(&hash));
        }

        // change the oldest time to now
        assert!(nodeinfo.contains_wanted_block(&Hash::hash(0.to_string().as_bytes())));
        //add hash that triggers container pruning
        nodeinfo.insert_wanted_blocks(Hash::hash("test2".as_bytes()), max_node_wanted_blocks_size);

        //0 is present because because its timestamp has been updated with contains_wanted_block
        assert!(nodeinfo.contains_wanted_block(&Hash::hash(0.to_string().as_bytes())));

        //1 has been removed because it's the oldest.
        assert!(nodeinfo.contains_wanted_block(&Hash::hash(1.to_string().as_bytes())));

        //Other blocks are present.
        for index in 2..9 {
            let hash = Hash::hash(index.to_string().as_bytes());
            assert!(nodeinfo.contains_wanted_block(&hash));
        }
    }
}
