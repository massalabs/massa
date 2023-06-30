use std::{collections::VecDeque, thread::JoinHandle};

use massa_channel::{receiver::MassaReceiver, sender::MassaSender};
use massa_logging::massa_trace;
use massa_models::{block_id::BlockId, prehash::PreHashSet};
use massa_protocol_exports::PeerId;
use massa_protocol_exports::{ProtocolConfig, ProtocolError};
use massa_storage::Storage;
use tracing::{debug, info, warn};

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
    storage: Storage,
    saved_blocks: VecDeque<BlockId>,
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
                        BlockHandlerPropagationCommand::IntegratedBlock { block_id, storage } => {
                            massa_trace!(
                                "protocol.protocol_worker.process_command.integrated_block.begin",
                                { "block_id": block_id }
                            );
                            let header = {
                                let block = {
                                    let blocks = storage.read_blocks();
                                    blocks.get(&block_id).cloned()
                                };
                                if let Some(block) = block {
                                    self.storage.store_block(block.clone());
                                    self.saved_blocks.push_back(block.id);
                                    if self.saved_blocks.len()
                                        > self.config.max_known_blocks_saved_size
                                    {
                                        let block_id = self.saved_blocks.pop_front().unwrap();
                                        let mut ids_to_delete = PreHashSet::default();
                                        ids_to_delete.insert(block_id);
                                        self.storage.drop_block_refs(&ids_to_delete);
                                    }
                                    block.content.header.clone()
                                } else {
                                    warn!("Block {} not found in storage", &block_id);
                                    continue;
                                }
                            };
                            let peers_connected = self.active_connections.get_peer_ids_connected();
                            self.cache.write().update_cache(
                                peers_connected,
                                self.config
                                    .max_node_known_blocks_size
                                    .try_into()
                                    .expect("max_node_known_blocks_size is too big"),
                            );
                            {
                                let cache_read = self.cache.read();
                                for (peer_id, (blocks_known, _)) in
                                    cache_read.blocks_known_by_peer.iter()
                                {
                                    // peer that isn't asking for that block
                                    let cond = blocks_known.peek(&block_id);
                                    // if we don't know if that peer knows that hash or if we know it doesn't
                                    if !cond.map_or_else(|| false, |v| v.0) {
                                        massa_trace!("protocol.protocol_worker.process_command.integrated_block.send_header", { "peer_id": peer_id, "block_id": block_id});
                                        debug!(
                                            "Send block header for slot {} to peer {}",
                                            peer_id, header.content.slot
                                        );
                                        if let Err(err) = self.active_connections.send_to_peer(
                                            peer_id,
                                            &self.block_serializer,
                                            BlockMessage::BlockHeader(header.clone()).into(),
                                            true,
                                        ) {
                                            warn!("Error while sending block header to peer {} err: {:?}", peer_id, err);
                                        }
                                    } else {
                                        massa_trace!("protocol.protocol_worker.process_command.integrated_block.do_not_send", { "peer_id": peer_id, "block_id": block_id });
                                    }
                                }
                            }
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
    storage: Storage,
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
                storage,
                saved_blocks: VecDeque::default(),
            };
            propagation_thread.run();
        })
        .expect("OS failed to start block propagation thread")
}
