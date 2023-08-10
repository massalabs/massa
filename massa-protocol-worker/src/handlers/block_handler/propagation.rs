//! Copyright (c) 2023 MASSA LABS <info@massa.net>

//! This file deals with the announcement of block headers to other nodes
//! in order to propagate the blocks from our node to other nodes.
//! It also manages peer banning for invalid blocks detected by consensus.
//!
//! The block propagation system works in the following way:
//! * a node announces the headers of blocks to its neighbor nodes
//! * the neighbor nodes that need that block then ask our Retrieval process for it
//!
//! Here we need to announce block headers to other nodes that haven't sene them,
//! and keep the blocks alive long enough for our peers to be able to retrieve them from us.

use super::{
    cache::SharedBlockCache, commands_propagation::BlockHandlerPropagationCommand,
    BlockMessageSerializer,
};
use crate::{
    handlers::{block_handler::BlockMessage, peer_handler::models::PeerManagementCmd},
    messages::MessagesSerializer,
    wrap_network::ActiveConnectionsTrait,
};
use crossbeam::channel::RecvTimeoutError;
use massa_channel::{receiver::MassaReceiver, sender::MassaSender};
use massa_models::block_header::SecuredHeader;
use massa_models::block_id::BlockId;
use massa_protocol_exports::PeerId;
use massa_protocol_exports::{ProtocolConfig, ProtocolError};
use massa_storage::Storage;
use schnellru::{ByLength, LruMap};
use std::thread::JoinHandle;
use std::time::Instant;
use tracing::{debug, info, warn};

#[derive(Debug)]
struct BlockPropagationData {
    /// Time when propagation was initiated
    pub time_added: Instant,
    /// Storage holding the block and its dependencies during its propagation time
    pub _storage: Storage,
    /// Clone of the block header to avoid locking storage during propagation
    pub header: SecuredHeader,
}

pub struct PropagationThread {
    /// Receiver for commands
    receiver: MassaReceiver<BlockHandlerPropagationCommand>,
    /// Protocol config
    config: ProtocolConfig,
    /// Shared access to the block cache
    cache: SharedBlockCache,
    /// Blocks stored for propagation
    stored_for_propagation: LruMap<BlockId, BlockPropagationData>,
    /// Shared access to the list of peers connected to us
    active_connections: Box<dyn ActiveConnectionsTrait>,
    /// Channel to send commands to the peer management system (for banning peers)
    peer_cmd_sender: MassaSender<PeerManagementCmd>,
    /// Serializer for block-related messages
    block_serializer: MessagesSerializer,
}

impl PropagationThread {
    fn run(&mut self) {
        let tick_interval = self.config.block_propagation_tick.to_duration();
        let mut deadline = Instant::now()
            .checked_add(tick_interval)
            .expect("could not get time of next propagation tick");
        loop {
            match self.receiver.recv_deadline(deadline) {
                Ok(command) => {
                    match command {
                        // Message: the block was integrated and should be propagated
                        BlockHandlerPropagationCommand::IntegratedBlock { block_id, storage } => {
                            debug!("received IntegratedBlock({})", block_id);

                            // get the block header
                            let header = match storage
                                .read_blocks()
                                .get(&block_id)
                                .map(|block| block.content.header.clone())
                            {
                                Some(h) => h,
                                None => {
                                    warn!(
                                        "claimed block {} absent from storage on propagation",
                                        block_id
                                    );
                                    continue;
                                }
                            };

                            // Add the block and its dependencies to the propagation LRU
                            // to ensure they are stored for the time of the propagation.
                            self.stored_for_propagation.insert(
                                block_id,
                                BlockPropagationData {
                                    time_added: Instant::now(),
                                    _storage: storage,
                                    header,
                                },
                            );

                            // propagate everything that needs to be propagated
                            self.perform_propagations();

                            // renew tick because propagation propagations were updated
                            deadline = Instant::now()
                                .checked_add(tick_interval)
                                .expect("could not get time of next propagation tick");
                        }
                        BlockHandlerPropagationCommand::AttackBlockDetected(block_id) => {
                            debug!("received AttackBlockDetected({})", block_id);
                            let peers_to_ban: Vec<PeerId> = self
                                .cache
                                .read()
                                .blocks_known_by_peer
                                .iter()
                                .filter_map(|(peer_id, knowledge)| {
                                    match knowledge.peek(&block_id) {
                                        Some((true, _)) => Some(peer_id.clone()),
                                        _ => None,
                                    }
                                })
                                .collect();
                            self.ban_peers(&peers_to_ban);
                        }
                        BlockHandlerPropagationCommand::Stop => {
                            info!("Stop block propagation thread");
                            return;
                        }
                    }
                }
                Err(RecvTimeoutError::Timeout) => {
                    // Propagation tick. This is useful to quickly propagate headers to newly connected nodes.
                    self.perform_propagations();
                    // renew deadline of next tick
                    deadline = Instant::now()
                        .checked_add(tick_interval)
                        .expect("could not get time of next propagation tick");
                }
                Err(RecvTimeoutError::Disconnected) => {
                    info!("Stop block propagation thread");
                    return;
                }
            }
        }
    }

    /// Propagate blocks to peers that need them
    fn perform_propagations(&mut self) {
        let now = Instant::now();

        // stop propagating blocks that have been propagating for too long
        while let Some(time_added) = self
            .stored_for_propagation
            .peek_oldest()
            .map(|(_, BlockPropagationData { time_added, .. })| *time_added)
        {
            if now.saturating_duration_since(time_added)
                > self.config.max_block_propagation_time.to_duration()
            {
                self.stored_for_propagation.pop_oldest();
            } else {
                break;
            }
        }

        // update caches based on currently connected peers
        let peers_connected = self.active_connections.get_peer_ids_connected();
        let mut cache_lock = self.cache.write();
        cache_lock.update_cache(&peers_connected);
        'peer_loop: for (peer_id, known_by_peer) in cache_lock.blocks_known_by_peer.iter_mut() {
            for (block_id, BlockPropagationData { header, .. }) in
                self.stored_for_propagation.iter()
            {
                // if the peer already knows about the block, do not propagate it
                if let Some((true, _)) = known_by_peer.peek(block_id) {
                    continue;
                }

                // try to propagate
                debug!("announcing header {} to peer {}", block_id, peer_id);
                match self.active_connections.send_to_peer(
                    peer_id,
                    &self.block_serializer,
                    BlockMessage::Header(header.clone()).into(),
                    true,
                ) {
                    Ok(()) => {
                        // mark the block as known by the peer
                        known_by_peer.insert(*block_id, (true, now));
                    }
                    Err(err) => {
                        warn!(
                            "Error while announcing block header {} to peer {} err: {:?}",
                            block_id, peer_id, err
                        );
                        continue 'peer_loop; // try next peer
                    }
                }
            }
        }
    }

    /// try to ban a list of peers
    fn ban_peers(&mut self, peer_ids: &[PeerId]) {
        if let Err(err) = self
            .peer_cmd_sender
            .try_send(PeerManagementCmd::Ban(peer_ids.to_vec()))
            .map_err(|err| ProtocolError::SendError(err.to_string()))
        {
            warn!("could not send Ban command to peer manager: {}", err);
        }
    }
}

pub fn start_propagation_thread(
    active_connections: Box<dyn ActiveConnectionsTrait>,
    receiver: MassaReceiver<BlockHandlerPropagationCommand>,
    peer_cmd_sender: MassaSender<PeerManagementCmd>,
    config: ProtocolConfig,
    cache: SharedBlockCache,
) -> JoinHandle<()> {
    std::thread::Builder::new()
        .name("protocol-block-handler-propagation".to_string())
        .spawn(move || {
            let block_serializer = MessagesSerializer::new()
                .with_block_message_serializer(BlockMessageSerializer::new());
            let mut propagation_thread = PropagationThread {
                stored_for_propagation: LruMap::new(ByLength::new(
                    config
                        .max_blocks_kept_for_propagation
                        .try_into()
                        .expect("max_blocks_kept_for_propagation does not fit in u32"),
                )),
                receiver,
                config,
                cache,
                peer_cmd_sender,
                active_connections,
                block_serializer,
            };
            propagation_thread.run();
        })
        .expect("OS failed to start block propagation thread")
}
