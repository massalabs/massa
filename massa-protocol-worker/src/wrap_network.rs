use std::{
    collections::{HashMap, HashSet},
    net::SocketAddr,
};

use massa_protocol_exports::{PeerId, ProtocolError};
use peernet::{
    network_manager::{PeerNetManager, SharedActiveConnections},
    peer::PeerConnectionType,
    transports::TransportType,
};

use crate::{
    context::Context,
    handlers::peer_handler::MassaHandshake,
    messages::{Message, MessagesHandler, MessagesSerializer},
};

pub trait ActiveConnectionsTrait: Send + Sync {
    fn send_to_peer(
        &self,
        peer_id: &PeerId,
        message_serializer: &MessagesSerializer,
        message: Message,
        high_priority: bool,
    ) -> Result<(), ProtocolError>;
    fn clone_box(&self) -> Box<dyn ActiveConnectionsTrait>;
    fn get_peer_ids_connected(&self) -> HashSet<PeerId>;
    fn get_peers_connected(
        &self,
    ) -> HashMap<PeerId, (SocketAddr, PeerConnectionType, Option<String>)>;
    fn get_nb_out_connections(&self) -> usize;
    fn get_nb_in_connections(&self) -> usize;
    fn shutdown_connection(&mut self, peer_id: &PeerId);
    fn get_peers_connections_bandwidth(&self) -> HashMap<String, (u64, u64)>;
}

impl Clone for Box<dyn ActiveConnectionsTrait> {
    fn clone(&self) -> Box<dyn ActiveConnectionsTrait> {
        self.clone_box()
    }
}

impl ActiveConnectionsTrait for SharedActiveConnections<PeerId> {
    fn send_to_peer(
        &self,
        peer_id: &PeerId,
        message_serializer: &MessagesSerializer,
        message: Message,
        high_priority: bool,
    ) -> Result<(), ProtocolError> {
        if let Some(connection) = self.read().connections.get(peer_id) {
            connection
                .send_channels
                .try_send(message_serializer, message, high_priority)
                .map_err(|err| ProtocolError::SendError(err.to_string()))
        } else {
            Err(ProtocolError::SendError(
                "Peer isn't connected anymore".to_string(),
            ))
        }
    }

    fn clone_box(&self) -> Box<dyn ActiveConnectionsTrait> {
        Box::new(self.clone())
    }

    fn get_peer_ids_connected(&self) -> HashSet<PeerId> {
        self.read().connections.keys().cloned().collect()
    }

    fn get_peers_connected(
        &self,
    ) -> HashMap<PeerId, (SocketAddr, PeerConnectionType, Option<String>)> {
        self.read()
            .connections
            .iter()
            .map(|(peer_id, connection)| {
                (
                    peer_id.clone(),
                    (
                        *connection.endpoint.get_target_addr(),
                        connection.connection_type,
                        connection.category_name.clone(),
                    ),
                )
            })
            .collect()
    }

    fn get_nb_out_connections(&self) -> usize {
        self.read().nb_out_connections
    }

    fn get_nb_in_connections(&self) -> usize {
        self.read().nb_in_connections
    }

    fn shutdown_connection(&mut self, peer_id: &PeerId) {
        if let Some(connection) = self.write().connections.get_mut(peer_id) {
            connection.shutdown();
        }
    }

    fn get_peers_connections_bandwidth(&self) -> HashMap<String, (u64, u64)> {
        let mut map = HashMap::new();
        for (peerid, conn) in self.read().connections.iter() {
            if let Ok(tuple) = conn.endpoint.get_bandwidth() {
                map.insert(peerid.to_string(), tuple);
            }
        }
        map
    }
}

pub trait NetworkController: Send + Sync {
    fn get_active_connections(&self) -> Box<dyn ActiveConnectionsTrait>;
    fn start_listener(
        &mut self,
        transport_type: TransportType,
        addr: SocketAddr,
    ) -> Result<(), ProtocolError>;
    fn stop_listener(
        &mut self,
        transport_type: TransportType,
        addr: SocketAddr,
    ) -> Result<(), ProtocolError>;
    fn try_connect(
        &mut self,
        addr: SocketAddr,
        timeout: std::time::Duration,
    ) -> Result<(), ProtocolError>;
    fn get_total_bytes_received(&self) -> Result<u64, ProtocolError>;
    fn get_total_bytes_sent(&self) -> Result<u64, ProtocolError>;
}

pub struct NetworkControllerImpl {
    peernet_manager: PeerNetManager<PeerId, Context, MassaHandshake, MessagesHandler>,
}

impl NetworkControllerImpl {
    pub fn new(
        peernet_manager: PeerNetManager<PeerId, Context, MassaHandshake, MessagesHandler>,
    ) -> Self {
        Self { peernet_manager }
    }
}

impl NetworkController for NetworkControllerImpl {
    fn get_active_connections(&self) -> Box<dyn ActiveConnectionsTrait> {
        Box::new(self.peernet_manager.active_connections.clone())
    }

    fn start_listener(
        &mut self,
        transport_type: TransportType,
        addr: SocketAddr,
    ) -> Result<(), ProtocolError> {
        self.peernet_manager
            .start_listener(transport_type, addr)
            .map_err(|err| ProtocolError::ListenerError(err.to_string()))
    }

    fn stop_listener(
        &mut self,
        transport_type: TransportType,
        addr: SocketAddr,
    ) -> Result<(), ProtocolError> {
        self.peernet_manager
            .stop_listener(transport_type, addr)
            .map_err(|err| ProtocolError::ListenerError(err.to_string()))
    }

    fn try_connect(
        &mut self,
        addr: SocketAddr,
        timeout: std::time::Duration,
    ) -> Result<(), ProtocolError> {
        //TODO: Change when we support multiple transports
        self.peernet_manager
            .try_connect(TransportType::Tcp, addr, timeout)
            .map_err(|err| ProtocolError::GeneralProtocolError(err.to_string()))?;
        Ok(())
    }

    fn get_total_bytes_received(&self) -> Result<u64, ProtocolError> {
        self.peernet_manager
            .get_total_bytes_received()
            .map_err(|err| ProtocolError::GeneralProtocolError(err.to_string()))
    }

    fn get_total_bytes_sent(&self) -> Result<u64, ProtocolError> {
        self.peernet_manager.get_total_bytes_sent().map_err(|err| {
            ProtocolError::GeneralProtocolError(format!(
                "Error while getting total bytes sent: {}",
                err
            ))
        })
    }
}
