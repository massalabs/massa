use std::{
    collections::HashMap,
    net::{IpAddr, SocketAddr},
    time::{SystemTime, UNIX_EPOCH},
};

use peernet::{
    error::PeerNetError,
    peer_id::PeerId,
    transports::TransportType,
    types::{Hash, KeyPair, Signature},
};

#[derive(Clone, Debug)]
pub struct Announcement {
    /// Listeners
    pub listeners: HashMap<SocketAddr, TransportType>,
    /// Timestamp
    pub timestamp: u128,
    /// serialized version
    serialized: Vec<u8>,
    /// Signature
    signature: Signature,
}

impl Announcement {
    pub fn new(
        listeners: HashMap<SocketAddr, TransportType>,
        keypair: &KeyPair,
    ) -> Result<Self, PeerNetError> {
        let mut buf: Vec<u8> = vec![];
        buf.extend(listeners.len().to_be_bytes());
        for listener in &listeners {
            let ip_bytes = match listener.0.ip() {
                IpAddr::V4(ip) => {
                    buf.push(4);
                    ip.octets().to_vec()
                }
                IpAddr::V6(ip) => {
                    buf.push(6);
                    ip.octets().to_vec()
                }
            };
            buf.extend_from_slice(&ip_bytes);
            let port_bytes = listener.0.port().to_be_bytes();
            buf.extend_from_slice(&port_bytes);
            buf.push(*listener.1 as u8);
        }
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("Time went backward")
            .as_millis();
        buf.extend(timestamp.to_be_bytes());
        Ok(Self {
            listeners,
            timestamp,
            signature: keypair
                .sign(&Hash::compute_from(&buf))
                .map_err(|err| PeerNetError::SignError(err.to_string()))?,
            serialized: buf,
        })
    }

    pub fn from_bytes(bytes: &[u8], peer_id: &PeerId) -> Result<Self, PeerNetError> {
        let mut listeners = HashMap::new();
        let nb_listeners = usize::from_be_bytes(bytes[..8].try_into().unwrap());
        let mut i = 8;
        for _ in 0..nb_listeners {
            let ip: IpAddr = match bytes[i] {
                4 => {
                    i += 1;
                    let bytes: [u8; 4] = bytes[i..i + 4].try_into().unwrap();
                    i += 4;
                    IpAddr::from(bytes)
                }
                6 => {
                    i += 1;
                    let bytes: [u8; 16] = bytes[i..i + 16].try_into().unwrap();
                    i += 16;
                    IpAddr::from(bytes)
                }
                _ => {
                    return Err(PeerNetError::HandshakeError(
                        "Invalid IP version".to_string(),
                    ))
                }
            };
            let port_bytes = bytes[i..i + 2].try_into().unwrap();
            i += 2;
            let port = u16::from_be_bytes(port_bytes);
            let transport_type = match bytes[i] {
                0 => TransportType::Tcp,
                1 => TransportType::Quic,
                _ => {
                    return Err(PeerNetError::HandshakeError(
                        "Invalid transport type".to_string(),
                    ))
                }
            };
            i += 1;
            let addr = SocketAddr::new(ip, port);
            listeners.insert(addr, transport_type);
        }
        let timestamp = u128::from_be_bytes(bytes[i..i + 16].try_into().unwrap());
        i += 16;
        let hash = Hash::compute_from(&bytes[..i]);
        let signature = Signature::from_bytes(bytes[i..].try_into().unwrap()).unwrap();
        if peer_id.verify_signature(&hash, &signature).is_ok() {
            Ok(Self {
                listeners,
                timestamp,
                signature,
                serialized: bytes[..i].to_vec(),
            })
        } else {
            Err(PeerNetError::HandshakeError(
                "Invalid signature".to_string(),
            ))
        }
    }

    pub fn to_bytes(&self) -> Vec<u8> {
        let mut bytes = self.serialized.clone();
        bytes.extend_from_slice(&self.signature.to_bytes());
        bytes
    }
}
