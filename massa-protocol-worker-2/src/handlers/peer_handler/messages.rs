use std::{
    collections::HashMap,
    net::{IpAddr, SocketAddr},
};

use peernet::{error::PeerNetError, peer_id::PeerId, transports::TransportType};

#[derive(Debug, Clone)]
pub enum PeerManagementMessage {
    // Receive the ip addresses sent by a peer when connecting.
    NewPeerConnected((PeerId, HashMap<SocketAddr, TransportType>)),
    // Receive the ip addresses sent by a peer that is already connected.
    ListPeers(Vec<(PeerId, HashMap<SocketAddr, TransportType>)>),
}

//TODO: Use a proper serialization system like we have in the rest of massa.
impl PeerManagementMessage {
    pub fn from_bytes(bytes: &[u8]) -> Result<Self, PeerNetError> {
        match bytes[0] {
            0 => {
                let peer_id = PeerId::from_bytes(&bytes[1..33].try_into().unwrap())?;
                let nb_listeners = u64::from_be_bytes(bytes[33..41].try_into().unwrap());
                let mut listeners = HashMap::with_capacity(nb_listeners as usize);
                let mut offset = 41;
                for _ in 0..nb_listeners {
                    let ip = match bytes[offset] {
                        4 => {
                            offset += 1;
                            let bytes: [u8; 4] = bytes[offset..offset + 4].try_into().unwrap();
                            offset += 4;
                            IpAddr::from(bytes)
                        }
                        6 => {
                            offset += 1;
                            let bytes: [u8; 16] = bytes[offset..offset + 16].try_into().unwrap();
                            offset += 16;
                            IpAddr::from(bytes)
                        }
                        _ => return Err(PeerNetError::InvalidMessage),
                    };
                    let port = u16::from_be_bytes(bytes[offset..offset + 2].try_into().unwrap());
                    offset += 2;
                    let transport_type = match bytes[offset] {
                        0 => TransportType::Tcp,
                        1 => TransportType::Quic,
                        _ => return Err(PeerNetError::InvalidMessage),
                    };
                    offset += 1;
                    listeners.insert(SocketAddr::new(ip, port), transport_type);
                }
                Ok(PeerManagementMessage::NewPeerConnected((
                    peer_id, listeners,
                )))
            }
            1 => {
                let nb_peers = u64::from_le_bytes(bytes[1..9].try_into().unwrap());
                let mut peers = Vec::with_capacity(nb_peers as usize);
                let mut offset = 9;
                for _ in 0..nb_peers {
                    let peer_id =
                        PeerId::from_bytes(&bytes[offset..offset + 32].try_into().unwrap())?;
                    offset += 32;
                    let nb_listeners =
                        u64::from_be_bytes(bytes[offset..offset + 8].try_into().unwrap());
                    offset += 8;
                    let mut listeners = HashMap::with_capacity(nb_listeners as usize);
                    for _ in 0..nb_listeners {
                        let ip = match bytes[offset] {
                            4 => {
                                offset += 1;
                                let bytes: [u8; 4] = bytes[offset..offset + 4].try_into().unwrap();
                                IpAddr::from(bytes)
                            }
                            6 => {
                                offset += 1;
                                let bytes: [u8; 16] =
                                    bytes[offset..offset + 16].try_into().unwrap();
                                IpAddr::from(bytes)
                            }
                            _ => return Err(PeerNetError::InvalidMessage),
                        };
                        offset += 16;
                        let port =
                            u16::from_be_bytes(bytes[offset..offset + 2].try_into().unwrap());
                        offset += 2;
                        let transport_type = match bytes[offset] {
                            0 => TransportType::Tcp,
                            1 => TransportType::Quic,
                            _ => return Err(PeerNetError::InvalidMessage),
                        };
                        offset += 1;
                        listeners.insert(SocketAddr::new(ip, port), transport_type);
                    }
                    peers.push((peer_id, listeners));
                }
                Ok(PeerManagementMessage::ListPeers(peers))
            }
            _ => Err(PeerNetError::InvalidMessage),
        }
    }

    pub fn to_bytes(&self) -> Vec<u8> {
        match self {
            PeerManagementMessage::NewPeerConnected((peer_id, listeners)) => {
                let mut bytes = vec![0];
                bytes.extend_from_slice(&peer_id.to_bytes());
                bytes.extend((listeners.len() as u64).to_be_bytes());
                for listener in listeners {
                    let ip_bytes = match listener.0.ip() {
                        IpAddr::V4(ip) => {
                            bytes.push(4);
                            ip.octets().to_vec()
                        }
                        IpAddr::V6(ip) => {
                            bytes.push(6);
                            ip.octets().to_vec()
                        }
                    };
                    bytes.extend_from_slice(&ip_bytes);
                    let port_bytes = listener.0.port().to_be_bytes();
                    bytes.extend_from_slice(&port_bytes);
                    bytes.push(*listener.1 as u8);
                }
                bytes
            }
            PeerManagementMessage::ListPeers(peers) => {
                let mut bytes = vec![1];
                let nb_peers = peers.len() as u64;
                bytes.extend_from_slice(&nb_peers.to_le_bytes());
                for (peer_id, listeners) in peers {
                    bytes.extend_from_slice(&peer_id.to_bytes());
                    bytes.extend(listeners.len().to_be_bytes());
                    for listener in listeners {
                        let ip_bytes = match listener.0.ip() {
                            IpAddr::V4(ip) => {
                                bytes.push(4);
                                ip.octets().to_vec()
                            }
                            IpAddr::V6(ip) => {
                                bytes.push(6);
                                ip.octets().to_vec()
                            }
                        };
                        bytes.extend_from_slice(&ip_bytes);
                        let port_bytes = listener.0.port().to_be_bytes();
                        bytes.extend_from_slice(&port_bytes);
                        bytes.push(*listener.1 as u8);
                    }
                }
                bytes
            }
        }
    }
}
