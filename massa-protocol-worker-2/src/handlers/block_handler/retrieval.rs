use std::thread::JoinHandle;

use crate::{handlers::peer_handler::models::PeerMessageTuple, messages::MessagesSerializer};
use crossbeam::{
    channel::{Receiver, Sender},
    select,
};
use massa_models::block_id::BlockId;
use massa_protocol_exports_2::{ProtocolConfig, ProtocolError};
use massa_serialization::{DeserializeError, Deserializer};
use massa_storage::Storage;
use peernet::{network_manager::SharedActiveConnections, peer_id::PeerId};
use tracing::warn;

use super::{
    cache::SharedBlockCache,
    commands_propagation::BlockHandlerCommand,
    commands_retrieval::BlockHandlerRetrievalCommand,
    messages::{
        AskForBlocksInfo, BlockInfoReply, BlockMessage, BlockMessageDeserializer,
        BlockMessageDeserializerArgs,
    },
    BlockMessageSerializer,
};

pub struct RetrievalThread {
    active_connections: SharedActiveConnections,
    receiver_network: Receiver<PeerMessageTuple>,
    internal_sender: Sender<BlockHandlerCommand>,
    receiver: Receiver<BlockHandlerRetrievalCommand>,
    block_message_serializer: MessagesSerializer,
    cache: SharedBlockCache,
    config: ProtocolConfig,
    storage: Storage,
}

impl RetrievalThread {
    fn run(&mut self) {
        //TODO: Add real values
        let mut block_message_deserializer =
            BlockMessageDeserializer::new(BlockMessageDeserializerArgs {
                thread_count: 32,
                endorsement_count: 10000,
                block_infos_length_max: 10000,
                max_operations_per_block: 10000,
                max_datastore_value_length: 10000,
                max_function_name_length: 10000,
                max_parameters_size: 10000,
                max_op_datastore_entry_count: 10000,
                max_op_datastore_key_length: 100,
                max_op_datastore_value_length: 10000,
            });
        loop {
            select! {
                recv(self.receiver_network) -> msg => {
                    match msg {
                        Ok((peer_id, message_id, message)) => {
                            block_message_deserializer.set_message_id(message_id);
                            let (rest, message) = block_message_deserializer
                                .deserialize::<DeserializeError>(&message)
                                .unwrap();
                            if !rest.is_empty() {
                                println!("Error: message not fully consumed");
                                return;
                            }
                            match message {
                                BlockMessage::AskForBlocks(block_infos) => {
                                    if let Err(err) = self.on_asked_for_blocks_received(peer_id, block_infos) {
                                        warn!("Error in on_asked_for_blocks_received: {:?}", err);
                                    }
                                }
                            }
                        },
                        Err(err) => {
                            println!("Error: {:?}", err);
                            return;
                        }
                    }
                },
                recv(self.receiver) -> msg => {
                    match msg {
                        Ok(command) => {
                        },
                        Err(err) => {
                            println!("Error: {:?}", err);
                            return;
                        }
                    }
                }
            }
        }
    }

    /// Network ask the local node for blocks
    ///
    /// React on another node asking for blocks information. We can forward the operation ids if
    /// the foreign node asked for `AskForBlocksInfo::Info` or the full operations if he asked for
    /// the missing operations in his storage with `AskForBlocksInfo::Operations`
    ///
    /// Forward the reply to the network.
    fn on_asked_for_blocks_received(
        &mut self,
        from_peer_id: PeerId,
        list: Vec<(BlockId, AskForBlocksInfo)>,
    ) -> Result<(), ProtocolError> {
        // let node_info = match self.active_nodes.get_mut(&from_node_id) {
        //     Some(node_info) => node_info,
        //     _ => return Ok(()),
        // };
        let mut all_blocks_info = vec![];
        for (hash, info_wanted) in &list {
            let (header, operations_ids) = match self.storage.read_blocks().get(hash) {
                Some(signed_block) => (
                    signed_block.content.header.clone(),
                    signed_block.content.operations.clone(),
                ),
                None => {
                    // let the node know we don't have the block.
                    all_blocks_info.push((*hash, BlockInfoReply::NotFound));
                    continue;
                }
            };
            let block_info = match info_wanted {
                AskForBlocksInfo::Header => BlockInfoReply::Header(header),
                AskForBlocksInfo::Info => BlockInfoReply::Info(operations_ids),
                AskForBlocksInfo::Operations(op_ids) => {
                    // Mark the node as having the block.
                    node_info.insert_known_blocks(
                        &[*hash],
                        true,
                        Instant::now(),
                        self.config.max_node_known_blocks_size,
                    );

                    // Send only the missing operations that are in storage.
                    let needed_ops = {
                        let operations = self.storage.read_operations();
                        operations_ids
                            .into_iter()
                            .filter(|id| op_ids.contains(id))
                            .filter_map(|id| operations.get(&id))
                            .cloned()
                            .collect()
                    };
                    BlockInfoReply::Operations(needed_ops)
                }
            };
            all_blocks_info.push((*hash, block_info));
        }
        {
            let read_active_connections = self.active_connections.read();
            let connection = read_active_connections
                .connections
                .get(&from_peer_id)
                .ok_or(ProtocolError::SendError(format!(
                    "Send block info peer {} isn't connected anymore",
                    &from_peer_id
                )))?;
            //connection.send_channels.send(, message, high_priority)
            self.network_command_sender
                .send_block_info(from_peer_id, all_blocks_info)
                .await
                .map_err(|_| {
                    ProtocolError::ChannelError(
                        "send block info network command send failed".into(),
                    )
                })
        }
    }
}

pub fn start_retrieval_thread(
    active_connections: SharedActiveConnections,
    receiver_network: Receiver<PeerMessageTuple>,
    receiver: Receiver<BlockHandlerRetrievalCommand>,
    internal_sender: Sender<BlockHandlerCommand>,
    config: ProtocolConfig,
    cache: SharedBlockCache,
    storage: Storage,
) -> JoinHandle<()> {
    let block_message_serializer =
        MessagesSerializer::new().with_block_message_serializer(BlockMessageSerializer::new());
    std::thread::spawn(move || {
        let mut retrieval_thread = RetrievalThread {
            active_connections,
            receiver_network,
            block_message_serializer,
            receiver,
            internal_sender,
            cache,
            config,
            storage,
        };
        retrieval_thread.run();
    })
}
