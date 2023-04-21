use std::{collections::HashSet, num::NonZeroUsize, thread::JoinHandle, time::Instant};

use crossbeam::channel::{Receiver, Sender};
use lru::LruCache;
use massa_logging::massa_trace;
use massa_protocol_exports_2::{ProtocolConfig, ProtocolError};
use peernet::{network_manager::SharedActiveConnections, peer_id::PeerId};
use tracing::warn;

use crate::{
    handlers::{block_handler::BlockMessage, peer_handler::models::PeerManagementCmd},
    messages::MessagesSerializer,
};

use super::{
    cache::SharedBlockCache, commands_propagation::BlockHandlerCommand, BlockMessageSerializer,
};

pub struct PropagationThread {
    receiver: Receiver<BlockHandlerCommand>,
    config: ProtocolConfig,
    cache: SharedBlockCache,
    active_connections: SharedActiveConnections,
    peer_cmd_sender: Sender<PeerManagementCmd>,
    block_serializer: MessagesSerializer,
}

impl PropagationThread {
    fn run(&mut self) {
        loop {
            match self.receiver.recv() {
                Ok(command) => {
                    match command {
                        BlockHandlerCommand::IntegratedBlock { block_id, storage } => {
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
                                    })
                                    .unwrap()
                            };
                            // Clean shared cache if peers do not exist anymore
                            {
                                let mut cache_write = self.cache.write();
                                let peers: Vec<PeerId> = cache_write
                                    .blocks_known_by_peer
                                    .iter()
                                    .map(|(id, _)| id.clone())
                                    .collect();
                                {
                                    let active_connections_read = self.active_connections.read();
                                    for peer_id in peers {
                                        if !active_connections_read
                                            .connections
                                            .contains_key(&peer_id)
                                        {
                                            cache_write.blocks_known_by_peer.pop(&peer_id);
                                        }
                                    }
                                }
                                let peers_connected: HashSet<PeerId> = self
                                    .active_connections
                                    .read()
                                    .connections
                                    .keys()
                                    .cloned()
                                    .collect();
                                for peer_id in peers_connected {
                                    if !cache_write.blocks_known_by_peer.contains(&peer_id) {
                                        //TODO: Change to detect the connection before
                                        cache_write.blocks_known_by_peer.put(
                                            peer_id,
                                            (
                                                LruCache::new(
                                                    NonZeroUsize::new(self.config.max_node_known_blocks_size)
                                                        .expect("max_node_known_blocks_size in config must be > 0"),
                                                ),
                                                Instant::now(),
                                            ),
                                        );
                                    }
                                }
                            }
                            {
                                let cache_read = self.cache.read();
                                for (peer_id, (blocks_known, _)) in &cache_read.blocks_known_by_peer
                                {
                                    // peer that isn't asking for that block
                                    let cond = blocks_known.peek(&block_id);
                                    // if we don't know if that peer knows that hash or if we know it doesn't
                                    if !cond.map_or_else(|| false, |v| v.0) {
                                        massa_trace!("protocol.protocol_worker.process_command.integrated_block.send_header", { "peer_id": peer_id, "block_id": block_id});
                                        if let Some(connection) =
                                            self.active_connections.read().connections.get(&peer_id)
                                        {
                                            if let Err(err) = connection.send_channels.send(
                                                &self.block_serializer,
                                                BlockMessage::BlockHeader(header.clone()).into(),
                                                true,
                                            ) {
                                                warn!("Error while sending block header to peer {} err: {:?}", peer_id, err);
                                            }
                                        }
                                    } else {
                                        massa_trace!("protocol.protocol_worker.process_command.integrated_block.do_not_send", { "peer_id": peer_id, "block_id": block_id });
                                    }
                                }
                            }
                        }
                        BlockHandlerCommand::AttackBlockDetected(block_id) => {
                            //TODO: Ban all nodes that sent us this object
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
                                let _ = self.ban_node(id);
                            }
                        }
                    }
                }
                Err(err) => {
                    println!("Error: {:?}", err)
                }
            }
        }
    }

    /// send a ban peer command to the peer handler
    fn ban_node(&mut self, peer_id: &PeerId) -> Result<(), ProtocolError> {
        massa_trace!("ban node from retrieval thread", { "peer_id": peer_id.to_string() });
        self.peer_cmd_sender
            .send(PeerManagementCmd::Ban(peer_id.clone()))
            .map_err(|err| ProtocolError::SendError(err.to_string()))
    }
}

pub fn start_propagation_thread(
    active_connections: SharedActiveConnections,
    receiver: Receiver<BlockHandlerCommand>,
    peer_cmd_sender: Sender<PeerManagementCmd>,
    config: ProtocolConfig,
    cache: SharedBlockCache,
) -> JoinHandle<()> {
    //TODO: Here and everywhere add id to threads
    std::thread::spawn(move || {
        let block_serializer =
            MessagesSerializer::new().with_block_message_serializer(BlockMessageSerializer::new());
        let mut propagation_thread = PropagationThread {
            receiver,
            config,
            cache,
            peer_cmd_sender,
            active_connections,
            block_serializer,
        };
        propagation_thread.run();
    })
}
