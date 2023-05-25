use std::{collections::HashMap, net::SocketAddr, ops::Bound::Included};

use massa_models::serialization::{IpAddrDeserializer, IpAddrSerializer};
use massa_protocol_exports::{PeerId, PeerIdDeserializer, PeerIdSerializer};
use massa_serialization::{
    Deserializer, SerializeError, Serializer, U64VarIntDeserializer, U64VarIntSerializer,
};
use nom::{
    error::{context, ContextError, ParseError},
    multi::length_count,
    sequence::tuple,
    IResult, Parser,
};
use num_enum::{IntoPrimitive, TryFromPrimitive};
use peernet::transports::TransportType;

#[derive(Debug, Clone)]
//TODO: Fix this clippy warning
#[allow(clippy::large_enum_variant)]
pub enum PeerManagementMessage {
    // Receive the ip addresses sent by a peer when connecting.
    NewPeerConnected((PeerId, HashMap<SocketAddr, TransportType>)),
    // Receive the ip addresses sent by a peer that is already connected.
    ListPeers(Vec<(PeerId, HashMap<SocketAddr, TransportType>)>),
}

#[derive(IntoPrimitive, Debug, Eq, PartialEq, TryFromPrimitive)]
#[repr(u64)]
pub enum MessageTypeId {
    NewPeerConnected = 0,
    ListPeers = 1,
}

impl From<&PeerManagementMessage> for MessageTypeId {
    fn from(message: &PeerManagementMessage) -> Self {
        match message {
            PeerManagementMessage::NewPeerConnected(_) => MessageTypeId::NewPeerConnected,
            PeerManagementMessage::ListPeers(_) => MessageTypeId::ListPeers,
        }
    }
}

#[derive(Default, Clone)]
pub struct PeerManagementMessageSerializer {
    id_serializer: U64VarIntSerializer,
    length_serializer: U64VarIntSerializer,
    ip_addr_serializer: IpAddrSerializer,
    peer_id_serializer: PeerIdSerializer,
}

impl PeerManagementMessageSerializer {
    pub fn new() -> Self {
        Self {
            id_serializer: U64VarIntSerializer::new(),
            length_serializer: U64VarIntSerializer::new(),
            ip_addr_serializer: IpAddrSerializer::new(),
            peer_id_serializer: PeerIdSerializer::new(),
        }
    }
}

impl Serializer<PeerManagementMessage> for PeerManagementMessageSerializer {
    fn serialize(
        &self,
        value: &PeerManagementMessage,
        buffer: &mut Vec<u8>,
    ) -> Result<(), massa_serialization::SerializeError> {
        self.id_serializer.serialize(
            &MessageTypeId::from(value).try_into().map_err(|_| {
                SerializeError::GeneralError(String::from("Failed to serialize id"))
            })?,
            buffer,
        )?;
        match value {
            PeerManagementMessage::NewPeerConnected((peer_id, listeners)) => {
                self.peer_id_serializer.serialize(peer_id, buffer)?;
                self.length_serializer
                    .serialize(&(listeners.len() as u64), buffer)?;
                for (socket_addr, transport_type) in listeners {
                    self.ip_addr_serializer
                        .serialize(&socket_addr.ip(), buffer)?;
                    buffer.extend_from_slice(&socket_addr.port().to_be_bytes());
                    buffer.push(*transport_type as u8);
                }
            }
            PeerManagementMessage::ListPeers(peers) => {
                self.length_serializer
                    .serialize(&(peers.len() as u64), buffer)?;
                for (peer_id, listeners) in peers {
                    self.peer_id_serializer.serialize(peer_id, buffer)?;
                    self.length_serializer
                        .serialize(&(listeners.len() as u64), buffer)?;
                    for (socket_addr, transport_type) in listeners {
                        self.ip_addr_serializer
                            .serialize(&socket_addr.ip(), buffer)?;
                        buffer.extend_from_slice(&socket_addr.port().to_be_bytes());
                        buffer.push(*transport_type as u8);
                    }
                }
            }
        }
        Ok(())
    }
}

pub struct PeerManagementMessageDeserializer {
    id_deserializer: U64VarIntDeserializer,
    listeners_length_deserializer: U64VarIntDeserializer,
    peers_length_deserializer: U64VarIntDeserializer,
    ip_addr_deserializer: IpAddrDeserializer,
    peer_id_deserializer: PeerIdDeserializer,
}

/// Limits used in the deserialization of `OperationMessage`
pub struct PeerManagementMessageDeserializerArgs {
    /// Maximum number of listeners per peer
    pub max_listeners_per_peer: u64,
    /// Maximum number of peers per announcement
    pub max_peers_per_announcement: u64,
}

impl PeerManagementMessageDeserializer {
    pub fn new(limits: PeerManagementMessageDeserializerArgs) -> Self {
        Self {
            id_deserializer: U64VarIntDeserializer::new(Included(0), Included(u64::MAX)),
            listeners_length_deserializer: U64VarIntDeserializer::new(
                Included(0),
                Included(limits.max_listeners_per_peer),
            ),
            peers_length_deserializer: U64VarIntDeserializer::new(
                Included(0),
                Included(limits.max_peers_per_announcement),
            ),
            ip_addr_deserializer: IpAddrDeserializer::new(),
            peer_id_deserializer: PeerIdDeserializer::new(),
        }
    }
}

impl Deserializer<PeerManagementMessage> for PeerManagementMessageDeserializer {
    fn deserialize<'a, E: ParseError<&'a [u8]> + ContextError<&'a [u8]>>(
        &self,
        buffer: &'a [u8],
    ) -> IResult<&'a [u8], PeerManagementMessage, E> {
        context("Failed PeerManagementMessage deserialization", |buffer| {
            let (buffer, raw_id) = self.id_deserializer.deserialize(buffer)?;
            let id = MessageTypeId::try_from(raw_id).map_err(|_| {
                nom::Err::Error(ParseError::from_error_kind(
                    buffer,
                    nom::error::ErrorKind::Eof,
                ))
            })?;
            match id {
                MessageTypeId::NewPeerConnected => context(
                    "Failed NewPeerConnected deserialization",
                    tuple((
                        context("Failed PeerId deserialization", |buffer: &'a [u8]| {
                            self.peer_id_deserializer.deserialize(buffer)
                        }),
                        length_count(
                            context("Failed length listeners deserialization", |buffer| {
                                self.listeners_length_deserializer.deserialize(buffer)
                            }),
                            context("Failed listener deserialization", |buffer| {
                                listener_deserializer(buffer, &self.ip_addr_deserializer)
                            }),
                        ),
                    )),
                )
                .map(
                    |(peer_id, listeners): (PeerId, Vec<(SocketAddr, TransportType)>)| {
                        let listeners = listeners.into_iter().collect();
                        PeerManagementMessage::NewPeerConnected((peer_id, listeners))
                    },
                )
                .parse(buffer),
                MessageTypeId::ListPeers => context(
                    "Failed ListPeers deserialization",
                    length_count(
                        context(
                            "Failed length peers deserialization",
                            |buffer: &'a [u8]| self.peers_length_deserializer.deserialize(buffer),
                        ),
                        context(
                            "Failed peer deserialization",
                            tuple((
                                context("Failed PeerId deserialization", |buffer: &'a [u8]| {
                                    self.peer_id_deserializer.deserialize(buffer)
                                }),
                                length_count(
                                    context("Failed length listeners deserialization", |buffer| {
                                        self.listeners_length_deserializer.deserialize(buffer)
                                    }),
                                    context("Failed listener deserialization", |buffer| {
                                        listener_deserializer(buffer, &self.ip_addr_deserializer)
                                    }),
                                )
                                .map::<_, HashMap<SocketAddr, TransportType>>(
                                    |listeners: Vec<(SocketAddr, TransportType)>| {
                                        listeners.into_iter().collect()
                                    },
                                ),
                            )),
                        ),
                    ),
                )
                .map(|data: Vec<(PeerId, HashMap<SocketAddr, TransportType>)>| {
                    PeerManagementMessage::ListPeers(data)
                })
                .parse(buffer),
            }
        })
        .parse(buffer)
    }
}

fn listener_deserializer<'a, E: ParseError<&'a [u8]> + ContextError<&'a [u8]>>(
    buffer: &'a [u8],
    ip_addr_deserializer: &IpAddrDeserializer,
) -> IResult<&'a [u8], (SocketAddr, TransportType), E> {
    context("Failed listener deserialization", |buffer| {
        tuple((
            |buffer| {
                context("Failed SocketAddr deserialization", |buffer| {
                    let (buffer, ip) = ip_addr_deserializer.deserialize(buffer)?;
                    let (buffer, port) = nom::number::complete::be_u16(buffer)?;
                    Ok((buffer, SocketAddr::new(ip, port)))
                })
                .parse(buffer)
            },
            |buffer| {
                context(
                    "Failed TransportType deserialization",
                    |buffer: &'a [u8]| {
                        if buffer.is_empty() {
                            return Err(nom::Err::Error(ParseError::from_error_kind(
                                buffer,
                                nom::error::ErrorKind::Eof,
                            )));
                        }
                        let transport_type = match buffer[0] {
                            0 => TransportType::Tcp,
                            1 => TransportType::Quic,
                            _ => {
                                return Err(nom::Err::Error(ParseError::from_error_kind(
                                    buffer,
                                    nom::error::ErrorKind::Eof,
                                )))
                            }
                        };

                        let data = match buffer.get(1..) {
                            Some(data) => data,
                            None => {
                                return Err(nom::Err::Error(ParseError::from_error_kind(
                                    buffer,
                                    nom::error::ErrorKind::LengthValue,
                                )))
                            }
                        };

                        Ok((data, transport_type))
                    },
                )
                .parse(buffer)
            },
        ))
        .map(|(socket_addr, transport_type)| (socket_addr, transport_type))
        .parse(buffer)
    })
    .parse(buffer)
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use super::{
        PeerManagementMessage, PeerManagementMessageDeserializer,
        PeerManagementMessageDeserializerArgs, PeerManagementMessageSerializer,
    };
    use massa_protocol_exports::PeerId;
    use massa_serialization::{DeserializeError, Deserializer, Serializer};
    use massa_signature::KeyPair;
    use peernet::transports::TransportType;

    #[test]
    fn test_peer_connected() {
        let keypair = KeyPair::generate(0).unwrap();
        let mut listeners = HashMap::new();
        listeners.insert("127.0.0.1:33036".parse().unwrap(), TransportType::Tcp);
        listeners.insert("127.0.0.1:33035".parse().unwrap(), TransportType::Quic);

        let serializer = PeerManagementMessageSerializer::new();
        let mut buffer = vec![];
        let msg = PeerManagementMessage::NewPeerConnected((
            PeerId::from_public_key(keypair.get_public_key()),
            listeners.clone(),
        ))
        .into();

        serializer.serialize(&msg, &mut buffer).unwrap();

        let deserializer =
            PeerManagementMessageDeserializer::new(PeerManagementMessageDeserializerArgs {
                max_listeners_per_peer: 1000,
                max_peers_per_announcement: 1000,
            });
        let (rest, message) = deserializer
            .deserialize::<DeserializeError>(&buffer)
            .unwrap();
        assert!(rest.is_empty());
        match message {
            PeerManagementMessage::NewPeerConnected((peer_id, message_listeners)) => {
                assert_eq!(peer_id, PeerId::from_public_key(keypair.get_public_key()));
                assert_eq!(message_listeners.len(), 2);

                for (addr, transport) in listeners.iter() {
                    assert!(message_listeners.contains_key(addr));
                    assert_eq!(message_listeners.get(addr).unwrap(), transport);
                }
            }
            _ => panic!("Bad message deserialized"),
        }
    }

    #[test]
    fn test_list_peers() {
        let keypair1 = KeyPair::generate(0).unwrap();
        let mut listeners = HashMap::new();
        listeners.insert("127.0.0.1:33036".parse().unwrap(), TransportType::Tcp);
        let keypair2 = KeyPair::generate(0).unwrap();
        let message = PeerManagementMessage::ListPeers(vec![
            (
                PeerId::from_public_key(keypair1.get_public_key()),
                listeners.clone(),
            ),
            (
                PeerId::from_public_key(keypair2.get_public_key()),
                listeners.clone(),
            ),
        ]);

        let serializer = PeerManagementMessageSerializer::new();
        let mut buffer = vec![];
        serializer.serialize(&message, &mut buffer).unwrap();
        let deserializer =
            PeerManagementMessageDeserializer::new(PeerManagementMessageDeserializerArgs {
                max_listeners_per_peer: 1000,
                max_peers_per_announcement: 1000,
            });
        let (rest, message) = deserializer
            .deserialize::<DeserializeError>(&buffer)
            .unwrap();
        assert!(rest.is_empty());
        match message {
            PeerManagementMessage::ListPeers(peers) => {
                assert_eq!(peers.len(), 2);
                let ids_vec = vec![keypair1.get_public_key(), keypair2.get_public_key()];
                let iter = peers.iter().zip(ids_vec.iter());
                for ((peer_id, message_listeners), public_key) in iter {
                    assert_eq!(peer_id, &PeerId::from_public_key(*public_key));
                    assert_eq!(message_listeners.len(), 1);
                    for (addr, transport) in listeners.iter() {
                        assert!(message_listeners.contains_key(addr));
                        assert_eq!(message_listeners.get(addr).unwrap(), transport);
                    }
                }
            }
            _ => panic!("Bad message deserialized"),
        }
    }
}
