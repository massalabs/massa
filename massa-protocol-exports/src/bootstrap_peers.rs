use massa_models::serialization::{IpAddrDeserializer, IpAddrSerializer};
use massa_serialization::{
    Deserializer, SerializeError, Serializer, U16VarIntDeserializer, U16VarIntSerializer,
    U32VarIntDeserializer, U32VarIntSerializer,
};
use nom::{
    bytes::complete::take,
    error::{context, ContextError, ParseError},
    multi::length_count,
    sequence::tuple,
    IResult, Parser,
};
use peernet::peer_id::PeerId;
use peernet::transports::TransportType;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::net::SocketAddr;
use std::ops::Bound::Included;

/// Peer info provided in bootstrap
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PeerData {
    pub listeners: HashMap<SocketAddr, TransportType>,
    pub category: String,
}

/// Peers that are transmitted during bootstrap
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BootstrapPeers(pub Vec<(PeerId, HashMap<SocketAddr, TransportType>)>);

/// Serializer for `BootstrapPeers`
pub struct BootstrapPeersSerializer {
    u32_serializer: U32VarIntSerializer,
    ip_addr_serializer: IpAddrSerializer,
    port_serializer: U16VarIntSerializer,
}

impl BootstrapPeersSerializer {
    /// Creates a new `BootstrapPeersSerializer`
    pub fn new() -> Self {
        Self {
            u32_serializer: U32VarIntSerializer::new(),
            ip_addr_serializer: IpAddrSerializer::new(),
            port_serializer: U16VarIntSerializer::new(),
        }
    }
}

impl Default for BootstrapPeersSerializer {
    fn default() -> Self {
        Self::new()
    }
}

impl Serializer<BootstrapPeers> for BootstrapPeersSerializer {
    /// ```
    /// use massa_protocol_exports::{BootstrapPeers, PeerId, TransportType, BootstrapPeersSerializer};
    /// use massa_serialization::Serializer;
    /// use peernet::types::KeyPair;
    /// use std::collections::HashMap;
    /// use std::str::FromStr;
    ///
    /// let keypair1 = KeyPair::generate(0).unwrap();
    /// let mut peers = vec![];
    /// let mut listeners1 = HashMap::default();
    /// listeners1.insert("127.0.0.1:8080".parse().unwrap(), TransportType::Tcp);
    /// peers.push((PeerId::from_public_key(keypair1.get_public_key()), listeners1));
    /// let mut keypair2 = KeyPair::generate(0).unwrap();
    /// let mut listeners2 = HashMap::default();
    /// listeners2.insert("[::1]:8080".parse().unwrap(), TransportType::Tcp);
    /// peers.push((PeerId::from_public_key(keypair1.get_public_key()), listeners2));
    /// let mut serialized = Vec::new();
    /// let peers = BootstrapPeers(peers);
    /// let peers_serializer = BootstrapPeersSerializer::new();
    /// peers_serializer.serialize(&peers, &mut serialized).unwrap();
    /// ```
    fn serialize(
        &self,
        value: &BootstrapPeers,
        buffer: &mut Vec<u8>,
    ) -> Result<(), SerializeError> {
        let peers_count: u32 = value.0.len().try_into().map_err(|err| {
            SerializeError::NumberTooBig(format!(
                "too many peers blocks in BootstrapPeers: {}",
                err
            ))
        })?;
        self.u32_serializer.serialize(&peers_count, buffer)?;
        for (peer_id, listeners) in value.0.iter() {
            buffer.extend_from_slice(&peer_id.to_bytes());
            self.u32_serializer
                .serialize(&(listeners.len() as u32), buffer)?;
            for (addr, transport_type) in listeners.iter() {
                self.ip_addr_serializer.serialize(&addr.ip(), buffer)?;
                self.port_serializer.serialize(&addr.port(), buffer)?;
                buffer.push(*transport_type as u8);
            }
        }
        Ok(())
    }
}

/// Deserializer for `BootstrapPeers`
pub struct BootstrapPeersDeserializer {
    length_deserializer: U32VarIntDeserializer,
    length_listeners_deserializer: U32VarIntDeserializer,
    ip_addr_deserializer: IpAddrDeserializer,
    port_deserializer: U16VarIntDeserializer,
}

impl BootstrapPeersDeserializer {
    /// Creates a new `BootstrapPeersDeserializer`
    ///
    /// Arguments:
    ///
    /// * `max_peers`: maximum peers that can be serialized
    pub fn new(max_peers: u32, max_listeners_per_peer: u32) -> Self {
        Self {
            length_deserializer: U32VarIntDeserializer::new(Included(0), Included(max_peers)),
            length_listeners_deserializer: U32VarIntDeserializer::new(
                Included(0),
                Included(max_listeners_per_peer),
            ),
            ip_addr_deserializer: IpAddrDeserializer::new(),
            port_deserializer: U16VarIntDeserializer::new(Included(0), Included(u16::MAX)),
        }
    }
}

impl Deserializer<BootstrapPeers> for BootstrapPeersDeserializer {
    /// ```
    /// use massa_protocol_exports::{BootstrapPeers, PeerId, TransportType, BootstrapPeersSerializer, BootstrapPeersDeserializer};
    /// use massa_serialization::{Serializer, Deserializer, DeserializeError};
    /// use peernet::types::KeyPair;
    /// use std::collections::HashMap;
    /// use std::str::FromStr;
    ///
    /// let keypair1 = KeyPair::generate(0).unwrap();
    /// let mut peers = vec![];
    /// let mut listeners1 = HashMap::default();
    /// listeners1.insert("127.0.0.1:8080".parse().unwrap(), TransportType::Tcp);
    /// peers.push((PeerId::from_public_key(keypair1.get_public_key()), listeners1));
    /// let mut keypair2 = KeyPair::generate(0).unwrap();
    /// let mut listeners2 = HashMap::default();
    /// listeners2.insert("[::1]:8080".parse().unwrap(), TransportType::Tcp);
    /// peers.push((PeerId::from_public_key(keypair1.get_public_key()), listeners2));
    /// let mut serialized = Vec::new();
    /// let peers = BootstrapPeers(peers);
    /// let peers_serializer = BootstrapPeersSerializer::new();
    /// peers_serializer.serialize(&peers, &mut serialized).unwrap();
    /// let peers_deserializer = BootstrapPeersDeserializer::new(10, 10);
    /// let (rest, peers) = peers_deserializer.deserialize::<DeserializeError>(&serialized).unwrap();
    /// assert!(rest.is_empty());
    /// assert_eq!(peers, peers);
    /// ```
    fn deserialize<'a, E: ParseError<&'a [u8]> + ContextError<&'a [u8]>>(
        &self,
        buffer: &'a [u8],
    ) -> IResult<&'a [u8], BootstrapPeers, E> {
        length_count(
            context("Failed length deserialization", |input| {
                self.length_deserializer.deserialize(input)
            }),
            context("Failed Peer deserialization", |input| {
                tuple((
                    context("Failed PeerId deserialization", |input: &'a [u8]| {
                        let (rest, peer_id) = take(32usize)(input)?;
                        Ok((
                            rest,
                            PeerId::from_bytes(
                                // IMPORTANT TODO: this will fail before a peernet update
                                peer_id.try_into().map_err(|_| {
                                    nom::Err::Error(ParseError::from_error_kind(
                                        input,
                                        nom::error::ErrorKind::Count,
                                    ))
                                })?,
                            )
                            .map_err(|_| {
                                nom::Err::Error(ParseError::from_error_kind(
                                    input,
                                    nom::error::ErrorKind::Count,
                                ))
                            })?,
                        ))
                    }),
                    length_count(
                        context("Failed length deserialization", |input| {
                            self.length_listeners_deserializer.deserialize(input)
                        }),
                        context("Failed listener deserialization", |buffer: &'a [u8]| {
                            tuple((
                                tuple((
                                    context("Failed ip deserialization", |buffer| {
                                        self.ip_addr_deserializer.deserialize(buffer)
                                    }),
                                    context("Failed port deserialization", |buffer| {
                                        self.port_deserializer.deserialize(buffer)
                                    }),
                                ))
                                .map(|(addr, ip)| SocketAddr::new(addr, ip)),
                                context("Failed transport deserialization", |buffer| {
                                    let (rest, id) = nom::number::complete::be_u8(buffer)?;
                                    match id {
                                        0 => Ok((rest, TransportType::Tcp)),
                                        1 => Ok((rest, TransportType::Quic)),
                                        _ => Err(nom::Err::Error(ParseError::from_error_kind(
                                            buffer,
                                            nom::error::ErrorKind::MapRes,
                                        ))),
                                    }
                                }),
                            ))
                            .parse(buffer)
                        }),
                    )
                    .map(|listeners| {
                        listeners
                            .into_iter()
                            .collect::<HashMap<SocketAddr, TransportType>>()
                    }),
                ))
                .parse(input)
            }),
        )
        .map(BootstrapPeers)
        .parse(buffer)
    }
}
