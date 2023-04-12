use std::thread::JoinHandle;

use crossbeam::channel::{Receiver, Sender};
use massa_logging::massa_trace;
use massa_models::{
    operation::{OperationId, SecureShareOperation},
    prehash::{CapacityAllocator, PreHashMap, PreHashSet},
    secure_share::Id,
    slot::Slot,
    timeslots::get_block_slot_timestamp,
};
use massa_pool_exports::PoolController;
use massa_protocol_exports_2::{ProtocolConfig, ProtocolError};
use massa_serialization::{DeserializeError, Deserializer};
use massa_storage::Storage;
use massa_time::MassaTime;
use peernet::peer_id::PeerId;

use crate::sig_verifier::verify_sigs_batch;
use tracing::warn;

use super::{
    cache::SharedOperationCache,
    commands_propagation::OperationHandlerCommand,
    messages::{OperationMessage, OperationMessageDeserializer, OperationMessageDeserializerArgs},
};

pub struct RetrievalThread {
    receiver: Receiver<(PeerId, u64, Vec<u8>)>,
    pool_controller: Box<dyn PoolController>,
    cache: SharedOperationCache,
    storage: Storage,
    config: ProtocolConfig,
    internal_sender: Sender<OperationHandlerCommand>,
}

impl RetrievalThread {
    fn run(&mut self) {
        //TODO: Real values
        let mut operation_message_deserializer =
            OperationMessageDeserializer::new(OperationMessageDeserializerArgs {
                //TODO: Real value from config
                max_operations_prefix_ids: u32::MAX,
                max_operations: u32::MAX,
                max_datastore_value_length: u64::MAX,
                max_function_name_length: u16::MAX,
                max_parameters_size: u32::MAX,
                max_op_datastore_entry_count: u64::MAX,
                max_op_datastore_key_length: u8::MAX,
                max_op_datastore_value_length: u64::MAX,
            });
        loop {
            match self.receiver.recv() {
                Ok((peer_id, message_id, message)) => {
                    operation_message_deserializer.set_message_id(message_id);
                    let (rest, message) = operation_message_deserializer
                        .deserialize::<DeserializeError>(&message)
                        .unwrap();
                    if !rest.is_empty() {
                        println!("Error: message not fully consumed");
                        return;
                    }
                    match message {
                        OperationMessage::Operations(ops) => {
                            if let Err(err) = self.note_operations_from_peer(ops, &peer_id) {
                                warn!("peer {} sent us critically incorrect operation, which may be an attack attempt by the remote peer or a loss of sync between us and the remote peer. Err = {}", peer_id, err);
                                //TODO: Ban
                                //let _ = self.ban_node(&node_id).await;
                            }
                        }
                        _ => todo!(),
                    }
                }
                Err(err) => {
                    println!("Error: {:?}", err);
                    return;
                }
            }
        }
    }

    fn note_operations_from_peer(
        &mut self,
        operations: Vec<SecureShareOperation>,
        source_peer_id: &PeerId,
    ) -> Result<(), ProtocolError> {
        massa_trace!("protocol.protocol_worker.note_operations_from_peer", { "peer": source_peer_id, "operations": operations });
        let length = operations.len();
        let mut new_operations = PreHashMap::with_capacity(length);
        let mut received_ids = PreHashSet::with_capacity(length);
        for operation in operations {
            let operation_id = operation.id;
            if operation.serialized_size() > self.config.max_serialized_operations_size_per_block {
                return Err(ProtocolError::InvalidOperationError(format!(
                    "Operation {} exceeds max block size,  maximum authorized {} bytes but found {} bytes",
                    operation_id,
                    operation.serialized_size(),
                    self.config.max_serialized_operations_size_per_block
                )));
            };
            received_ids.insert(operation_id);

            // Check operation signature only if not already checked.
            if !self.cache.read().checked_operations.contains(&operation_id) {
                // check signature if the operation wasn't in `checked_operation`
                new_operations.insert(operation_id, operation);
            };
        }

        // optimized signature verification
        verify_sigs_batch(
            &new_operations
                .iter()
                .map(|(op_id, op)| (*op_id.get_hash(), op.signature, op.content_creator_pub_key))
                .collect::<Vec<_>>(),
        )?;

        {
            // add to checked operations
            let mut cache_write = self.cache.write();
            for op in new_operations.keys().copied() {
                cache_write.checked_operations.put(op, ());
            }

            // add to known ops
            if let Some(known_ops) = cache_write.ops_known_by_peer.get_mut(source_peer_id) {
                known_ops.extend(received_ids.iter().map(|id| id.prefix()));
            }
        }

        if !new_operations.is_empty() {
            // Store operation, claim locally
            let mut ops = self.storage.clone_without_refs();
            ops.store_operations(new_operations.into_values().collect());

            // Propagate operations when their expire period isn't `max_operations_propagation_time` old.
            let mut ops_to_propagate = ops.clone();
            let operations_to_not_propagate = {
                let now = MassaTime::now()?;
                let read_operations = ops_to_propagate.read_operations();
                ops_to_propagate
                    .get_op_refs()
                    .iter()
                    .filter(|op_id| {
                        let expire_period =
                            read_operations.get(op_id).unwrap().content.expire_period;
                        let expire_period_timestamp = get_block_slot_timestamp(
                            self.config.thread_count,
                            self.config.t0,
                            self.config.genesis_timestamp,
                            Slot::new(expire_period, 0),
                        );
                        match expire_period_timestamp {
                            Ok(slot_timestamp) => {
                                slot_timestamp
                                    .saturating_add(self.config.max_operations_propagation_time)
                                    < now
                            }
                            Err(_) => true,
                        }
                    })
                    .copied()
                    .collect()
            };
            ops_to_propagate.drop_operation_refs(&operations_to_not_propagate);
            let to_announce: PreHashSet<OperationId> =
                ops_to_propagate.get_op_refs().iter().copied().collect();
            self.internal_sender
                .send(OperationHandlerCommand::AnnounceOperations(to_announce))
                .map_err(|err| ProtocolError::SendError(err.to_string()))?;
            // Add to pool
            self.pool_controller.add_operations(ops);
        }

        Ok(())
    }
}

pub fn start_retrieval_thread(
    receiver: Receiver<(PeerId, u64, Vec<u8>)>,
    pool_controller: Box<dyn PoolController>,
    storage: Storage,
    config: ProtocolConfig,
    cache: SharedOperationCache,
    internal_sender: Sender<OperationHandlerCommand>,
) -> JoinHandle<()> {
    std::thread::spawn(move || {
        let mut retrieval_thread = RetrievalThread {
            receiver,
            pool_controller,
            storage,
            internal_sender,
            cache,
            config,
        };
        retrieval_thread.run();
    })
}
