use std::{
    collections::HashMap,
    net::{IpAddr, SocketAddr},
    ops::Bound::Included,
};

use massa_hash::Hash;
use massa_models::serialization::IpAddrDeserializer;
use massa_signature::{KeyPair, Signature, SignatureDeserializer};
use massa_time::MassaTime;
use nom::{
    error::{context, ContextError, ParseError},
    multi::length_count,
    sequence::tuple,
    IResult, Parser,
};
use peernet::{
    error::{PeerNetError, PeerNetResult},
    transports::TransportType,
};

use massa_serialization::{
    DeserializeError, Deserializer, SerializeError, Serializer, U64VarIntDeserializer,
    U64VarIntSerializer,
};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Announcement {
    /// Listeners
    pub listeners: HashMap<SocketAddr, TransportType>,
    /// Timestamp
    pub timestamp: u64,
    /// Hash
    pub hash: Hash,
    /// serialized version
    serialized: Vec<u8>,
    /// Signature
    pub signature: Signature,
}

#[derive(Clone)]
pub struct AnnouncementSerializer;

impl AnnouncementSerializer {
    pub fn new() -> Self {
        Self
    }
}

impl Serializer<Announcement> for AnnouncementSerializer {
    fn serialize(&self, value: &Announcement, buffer: &mut Vec<u8>) -> Result<(), SerializeError> {
        buffer.extend(value.serialized.clone());
        buffer.extend(value.signature.to_bytes());
        Ok(())
    }
}

#[derive(Clone)]
pub struct AnnouncementDeserializer {
    length_listeners_deserializer: U64VarIntDeserializer,
    ip_addr_deserializer: IpAddrDeserializer,
}

pub struct AnnouncementDeserializerArgs {
    pub max_listeners: u64,
}

impl AnnouncementDeserializer {
    pub fn new(args: AnnouncementDeserializerArgs) -> Self {
        Self {
            length_listeners_deserializer: U64VarIntDeserializer::new(
                Included(0),
                Included(args.max_listeners),
            ),
            ip_addr_deserializer: IpAddrDeserializer::new(),
        }
    }
}

impl Deserializer<Announcement> for AnnouncementDeserializer {
    fn deserialize<'a, E: ParseError<&'a [u8]> + ContextError<&'a [u8]>>(
        &self,
        buffer: &'a [u8],
    ) -> IResult<&'a [u8], Announcement, E> {
        let (rest, (listeners, timestamp)) = context(
            "Failed announcement deserialization",
            tuple((
                length_count(
                    context("Failed listeners deserialization", |buffer| {
                        self.length_listeners_deserializer.deserialize(buffer)
                    }),
                    context("Failed listener deserialization", |buffer: &'a [u8]| {
                        tuple((
                            tuple((
                                context("Failed ip deserialization", |buffer| {
                                    self.ip_addr_deserializer.deserialize(buffer)
                                }),
                                context("Failed port deserialization", |buffer| {
                                    nom::number::complete::be_u16(buffer)
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
                        ))(buffer)
                    }),
                ),
                context("Failed timestamp deserialization", |buffer: &'a [u8]| {
                    let timestamp = u64::from_be_bytes(buffer[..16].try_into().map_err(|_| {
                        nom::Err::Error(ParseError::from_error_kind(
                            buffer,
                            nom::error::ErrorKind::LengthValue,
                        ))
                    })?);
                    Ok((&buffer[16..], timestamp))
                }),
            )),
        )
        .map(|info| info)
        .parse(buffer)?;
        let serialized = buffer[..buffer.len() - rest.len()].to_vec();
        let hash = Hash::compute_from(&serialized);
        let signature_deserializer = SignatureDeserializer::new();
        let (rest, signature) = signature_deserializer
            .deserialize::<DeserializeError>(rest)
            .map_err(|_| {
                nom::Err::Error(ParseError::from_error_kind(
                    rest,
                    nom::error::ErrorKind::Verify,
                ))
            })?;
        Ok((
            rest,
            Announcement {
                listeners: listeners.into_iter().collect(),
                hash,
                timestamp,
                serialized,
                signature,
            },
        ))
    }
}

impl Announcement {
    pub fn new(
        mut listeners: HashMap<SocketAddr, TransportType>,
        routable_ip: Option<IpAddr>,
        keypair: &KeyPair,
    ) -> PeerNetResult<Self> {
        let mut buf: Vec<u8> = vec![];
        let length_serializer = U64VarIntSerializer::new();
        //TODO: Hacky to fix and adapt to support multiple ip/listeners
        if routable_ip.is_none() {
            listeners = HashMap::default()
        }
        length_serializer
            .serialize(&(listeners.len() as u64), &mut buf)
            .map_err(|err| {
                PeerNetError::HandlerError
                    .error("Announcement serialization", Some(err.to_string()))
            })?;
        for listener in &listeners {
            let ip = routable_ip.unwrap_or_else(|| listener.0.ip());
            let ip_bytes = match ip {
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
        let timestamp = MassaTime::now()
            .expect("Unable to get MassaTime::now")
            .to_millis();
        buf.extend(timestamp.to_be_bytes());
        let hash = Hash::compute_from(&buf);
        Ok(Self {
            listeners,
            timestamp,
            hash,
            signature: keypair.sign(&hash).map_err(|err| {
                PeerNetError::SignError.error("Announcement serialization", Some(err.to_string()))
            })?,
            serialized: buf,
        })
    }
}

#[cfg(test)]
mod tests {
    use crate::handlers::peer_handler::announcement::{
        Announcement, AnnouncementDeserializer, AnnouncementDeserializerArgs,
    };
    use massa_serialization::{DeserializeError, Deserializer, Serializer};
    use massa_signature::KeyPair;
    use peernet::transports::TransportType;
    use std::collections::HashMap;

    use super::AnnouncementSerializer;

    #[test]
    fn test_ser_deser() {
        let mut listeners = HashMap::new();
        listeners.insert("127.0.0.1:8081".parse().unwrap(), TransportType::Tcp);
        listeners.insert("127.0.0.1:8082".parse().unwrap(), TransportType::Quic);
        let announcement =
            Announcement::new(listeners, None, &KeyPair::generate(0).unwrap()).unwrap();
        let announcement_serializer = AnnouncementSerializer::new();
        let announcement_deserializer =
            AnnouncementDeserializer::new(AnnouncementDeserializerArgs { max_listeners: 100 });
        let mut buf: Vec<u8> = vec![];
        announcement_serializer
            .serialize(&announcement, &mut buf)
            .unwrap();
        let (_, announcement_deserialized) = announcement_deserializer
            .deserialize::<DeserializeError>(&buf)
            .unwrap();
        assert_eq!(announcement, announcement_deserialized);
    }
}
