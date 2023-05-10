use std::{
    collections::{HashMap, HashSet},
    sync::Arc,
};

use crossbeam::channel::{Receiver, Sender};
use massa_protocol_exports::ProtocolError;
use parking_lot::RwLock;
use peernet::{
    messages::{
        MessagesHandler as PeerNetMessagesHandler, MessagesSerializer as PeerNetMessagesSerializer,
    },
    peer::PeerConnectionType,
    peer_id::PeerId,
};

use crate::{
    handlers::{
        block_handler::BlockMessageSerializer, endorsement_handler::EndorsementMessageSerializer,
        operation_handler::OperationMessageSerializer,
        peer_handler::PeerManagementMessageSerializer,
    },
    messages::{Message, MessagesHandler, MessagesSerializer},
    wrap_network::{ActiveConnectionsTrait, NetworkController},
};

pub struct MockActiveConnections {
    pub connections: HashMap<PeerId, Sender<Message>>,
}

impl MockActiveConnections {
    pub fn new() -> Self {
        Self {
            connections: HashMap::new(),
        }
    }
}

type SharedMockActiveConnections = Arc<RwLock<MockActiveConnections>>;

impl ActiveConnectionsTrait for SharedMockActiveConnections {
    fn clone_box(&self) -> Box<dyn ActiveConnectionsTrait> {
        Box::new(self.clone())
    }

    fn get_nb_out_connections(&self) -> usize {
        //TODO: Place a coherent value
        0
    }

    fn get_nb_in_connections(&self) -> usize {
        //TODO: Place a coherent value
        0
    }

    fn get_peers_connected(
        &self,
    ) -> HashMap<PeerId, (std::net::SocketAddr, PeerConnectionType, Option<String>)> {
        self.read()
            .connections
            .iter()
            .map(|(peer_id, _)| {
                (
                    peer_id.clone(),
                    (
                        std::net::SocketAddr::from(([127, 0, 0, 1], 0)),
                        PeerConnectionType::OUT,
                        None,
                    ),
                )
            })
            .collect()
    }

    fn get_peer_ids_connected(&self) -> std::collections::HashSet<PeerId> {
        self.read().connections.keys().cloned().collect()
    }

    fn send_to_peer(
        &self,
        peer_id: &PeerId,
        _message_serializer: &crate::messages::MessagesSerializer,
        message: Message,
        _high_priority: bool,
    ) -> Result<(), massa_protocol_exports::ProtocolError> {
        let _ = self.read().connections.get(peer_id).unwrap().send(message);
        Ok(())
    }

    fn shutdown_connection(&mut self, peer_id: &PeerId) {
        self.write().connections.remove(peer_id);
    }
}

pub struct MockNetworkController {
    connections: SharedMockActiveConnections,
    messages_handler: MessagesHandler,
    message_serializer: MessagesSerializer,
}

impl Clone for MockNetworkController {
    fn clone(&self) -> Self {
        Self {
            connections: self.connections.clone(),
            messages_handler: self.messages_handler.clone(),
            message_serializer: MessagesSerializer::new()
                .with_block_message_serializer(BlockMessageSerializer::new())
                .with_endorsement_message_serializer(EndorsementMessageSerializer::new())
                .with_operation_message_serializer(OperationMessageSerializer::new())
                .with_peer_management_message_serializer(PeerManagementMessageSerializer::new()),
        }
    }
}

impl MockNetworkController {
    pub fn new(messages_handler: MessagesHandler) -> Self {
        Self {
            connections: Arc::new(RwLock::new(MockActiveConnections::new())),
            messages_handler,
            message_serializer: MessagesSerializer::new()
                .with_block_message_serializer(BlockMessageSerializer::new())
                .with_endorsement_message_serializer(EndorsementMessageSerializer::new())
                .with_operation_message_serializer(OperationMessageSerializer::new())
                .with_peer_management_message_serializer(PeerManagementMessageSerializer::new()),
        }
    }
}

impl MockNetworkController {
    pub fn create_fake_connection(&mut self, peer_id: PeerId) -> (PeerId, Receiver<Message>) {
        let (sender, receiver) = crossbeam::channel::unbounded();
        self.connections
            .write()
            .connections
            .insert(peer_id.clone(), sender);
        (peer_id, receiver)
    }

    pub fn remove_fake_connection(&mut self, peer_id: &PeerId) {
        self.connections.write().connections.remove(peer_id);
    }

    /// Simulate a peer that send a message to us
    pub fn send_from_peer(
        &mut self,
        peer_id: &PeerId,
        message: Message,
    ) -> Result<(), ProtocolError> {
        let peers_connected: HashSet<PeerId> = self
            .connections
            .read()
            .connections
            .keys()
            .cloned()
            .collect();
        if !peers_connected.contains(peer_id) {
            return Err(ProtocolError::GeneralProtocolError(
                "Peer not connected".to_string(),
            ));
        }
        let mut data = Vec::new();
        self.message_serializer
            .serialize_id(&message, &mut data)
            .map_err(|err| ProtocolError::GeneralProtocolError(err.to_string()))?;
        self.message_serializer
            .serialize(&message, &mut data)
            .map_err(|err| ProtocolError::GeneralProtocolError(err.to_string()))?;
        let (rest, id) = self
            .messages_handler
            .deserialize_id(&data, peer_id)
            .map_err(|err| ProtocolError::GeneralProtocolError(err.to_string()))?;
        self.messages_handler
            .handle(id, rest, peer_id)
            .map_err(|err| ProtocolError::GeneralProtocolError(err.to_string()))?;
        Ok(())
    }

    pub fn get_connections(&self) -> SharedMockActiveConnections {
        self.connections.clone()
    }
}

impl NetworkController for MockNetworkController {
    fn start_listener(
        &mut self,
        _transport_type: peernet::transports::TransportType,
        _addr: std::net::SocketAddr,
    ) -> Result<(), massa_protocol_exports::ProtocolError> {
        Ok(())
    }

    fn stop_listener(
        &mut self,
        _transport_type: peernet::transports::TransportType,
        _addr: std::net::SocketAddr,
    ) -> Result<(), massa_protocol_exports::ProtocolError> {
        Ok(())
    }

    fn try_connect(
        &mut self,
        _addr: std::net::SocketAddr,
        _timeout: std::time::Duration,
        _out_connection_config: &peernet::transports::OutConnectionConfig,
    ) -> Result<(), massa_protocol_exports::ProtocolError> {
        Ok(())
    }

    fn get_active_connections(&self) -> Box<dyn crate::wrap_network::ActiveConnectionsTrait> {
        Box::new(self.connections.clone())
    }
}
