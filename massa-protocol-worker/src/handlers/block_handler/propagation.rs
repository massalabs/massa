use std::time::Instant;
use std::{collections::VecDeque, thread::JoinHandle};

use massa_channel::{receiver::MassaReceiver, sender::MassaSender};
use massa_logging::massa_trace;
use massa_models::block_header::SecuredHeader;
use massa_models::block_id::BlockId;
use massa_protocol_exports::PeerId;
use massa_protocol_exports::{ProtocolConfig, ProtocolError};
use massa_storage::Storage;
use schnellru::LruMap;
use tracing::{info, warn};

use crate::{
    handlers::{block_handler::BlockMessage, peer_handler::models::PeerManagementCmd},
    messages::MessagesSerializer,
    wrap_network::ActiveConnectionsTrait,
};

use super::{
    cache::SharedBlockCache, commands_propagation::BlockHandlerPropagationCommand,
    BlockMessageSerializer,
};

pub struct PropagationThread {
    receiver: MassaReceiver<BlockHandlerPropagationCommand>,
    config: ProtocolConfig,
    cache: SharedBlockCache,
    /// Blocks stored for propagation
    /// Format: block_id => (time_added, storage referencing all the block data including header and ops and endorsements, clone of the header of the block)
    stored_for_propagation: LruMap<BlockId, (Instant, Storage, SecuredHeader)>,
    active_connections: Box<dyn ActiveConnectionsTrait>,
    peer_cmd_sender: MassaSender<PeerManagementCmd>,
    block_serializer: MessagesSerializer,
}

impl PropagationThread {
    fn run(&mut self) {
        loop {
            match self.receiver.recv() {
                Ok(command) => {
                    match command {
                        // Message: the block was integrated and should be propagated
                        BlockHandlerPropagationCommand::IntegratedBlock { block_id, storage } => {
                            massa_trace!(
                                "protocol.protocol_worker.process_command.integrated_block.begin",
                                { "block_id": block_id }
                            );

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
                            self.stored_for_propagation
                                .insert(block_id, (Instant::now(), storage, header));

                            // stop propagating blocks that are too old
                            loop {
                                match self
                                    .stored_for_propagation
                                    .peek_oldest()
                                    .map(|(_, (t, _, _))| *t)
                                {
                                    Some(time_added) => {
                                        if time_added.elapsed()
                                            > self.config.max_block_propagation_time.to_duration()
                                        {
                                            self.stored_for_propagation.pop_oldest();
                                        } else {
                                            break;
                                        }
                                    }
                                    None => break,
                                }
                            }

                            // propagate everything that needs to be propagated
                            self.perform_propagations();
                        }
                        BlockHandlerPropagationCommand::AttackBlockDetected(block_id) => {
                            let to_ban: Vec<PeerId> = self
                                .cache
                                .read()
                                .blocks_known_by_peer
                                .iter()
                                .filter_map(|(id, (block_known, _))| {
                                    match block_known.peek(&block_id) {
                                        Some((true, _)) => Some(id.clone()),
                                        _ => None,
                                    }
                                })
                                .collect();
                            for id in to_ban.iter() {
                                massa_trace!("protocol.protocol_worker.process_command.attack_block_detected.ban_node", { "node": id, "block_id": block_id });
                                if let Err(err) = self.ban_node(id) {
                                    warn!("Error while banning peer {} err: {:?}", id, err);
                                }
                            }
                        }
                        BlockHandlerPropagationCommand::Stop => {
                            info!("Stop block propagation thread");
                            return;
                        }
                    }
                }
                Err(_) => {
                    info!("Stop block propagation thread");
                    return;
                }
            }
        }
    }

    /// Propagate blocks to peers that need them
    fn perform_propagations(&mut self) {
        let now = Instant::now();
        // update caches based on currently connected peers
        let peers_connected = self.active_connections.get_peer_ids_connected();
        let cache_lock = self.cache.write();
        cache_lock.update_cache(&peers_connected);
        'peer_loop: for (peer_id, known_by_peer) in cache_lock.blocks_known_by_peer.iter_mut() {
            for (block_id, (added_time, _, header)) in self.stored_for_propagation.iter() {
                // if the peer already knows about the block, do not propagate it
                if let Some((true, _)) = known_by_peer.get(block_id) {
                    continue;
                }

                // try to propagate
                match self.active_connections.send_to_peer(
                    peer_id,
                    &self.block_serializer,
                    BlockMessage::BlockHeader(header.clone()).into(),
                    true,
                ) {
                    Ok(()) => {
                        // mark the block as known by the peer
                        known_by_peer.insert(*block_id, (true, now));
                    }
                    Err(err) => {
                        warn!(
                            "Error while sending block header to peer {} err: {:?}",
                            peer_id, err
                        );
                        continue 'peer_loop; // try next peer
                    }
                }
            }
        }
    }

    /// send a ban peer command to the peer handler
    fn ban_node(&mut self, peer_id: &PeerId) -> Result<(), ProtocolError> {
        massa_trace!("ban node from retrieval thread", { "peer_id": peer_id.to_string() });
        self.peer_cmd_sender
            .try_send(PeerManagementCmd::Ban(vec![peer_id.clone()]))
            .map_err(|err| ProtocolError::SendError(err.to_string()))
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
                receiver,
                config,
                cache,
                peer_cmd_sender,
                active_connections,
                block_serializer,
                stored_for_propagation: VecDeque::default(), //TODO initialize with a config-defined max_propagation_store_length
            };
            propagation_thread.run();
        })
        .expect("OS failed to start block propagation thread")
}
