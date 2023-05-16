use std::{fmt::Display, hash::Hash, str::FromStr};

use massa_hash::Hash as MassaHash;
use massa_serialization::{Deserializer, Serializer};
use massa_signature::{KeyPair, PublicKey, PublicKeyDeserializer, Signature};
use peernet::peer_id::PeerId as PeernetPeerId;

use crate::ProtocolError;

#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct PeerId {
    public_key: PublicKey,
}

impl PeerId {
    pub fn from_public_key(public_key: PublicKey) -> Self {
        Self { public_key }
    }

    pub fn get_public_key(&self) -> PublicKey {
        self.public_key.clone()
    }

    pub fn verify_signature(
        &self,
        hash: &MassaHash,
        signature: &Signature,
    ) -> Result<(), ProtocolError> {
        self.public_key
            .verify_signature(hash, signature)
            .map_err(|err| ProtocolError::GeneralProtocolError(err.to_string()))
    }
}

impl FromStr for PeerId {
    type Err = ProtocolError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let public_key = PublicKey::from_str(s)
            .map_err(|err| ProtocolError::GeneralProtocolError(err.to_string()))?;
        Ok(Self { public_key })
    }
}

impl Display for PeerId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.public_key.fmt(f)
    }
}

impl PeernetPeerId for PeerId {
    fn generate() -> Self {
        Self {
            public_key: KeyPair::generate(0).unwrap().get_public_key(),
        }
    }
}

#[derive(Default, Clone)]
pub struct PeerIdSerializer {}

impl PeerIdSerializer {
    pub fn new() -> Self {
        Self {}
    }
}

impl Serializer<PeerId> for PeerIdSerializer {
    fn serialize(
        &self,
        value: &PeerId,
        buffer: &mut Vec<u8>,
    ) -> Result<(), massa_serialization::SerializeError> {
        buffer.extend_from_slice(&value.public_key.to_bytes());
        Ok(())
    }
}

#[derive(Default, Clone)]
pub struct PeerIdDeserializer {
    public_key_deserializer: PublicKeyDeserializer,
}

impl PeerIdDeserializer {
    pub fn new() -> Self {
        PeerIdDeserializer {
            public_key_deserializer: PublicKeyDeserializer::new(),
        }
    }
}

impl Deserializer<PeerId> for PeerIdDeserializer {
    fn deserialize<'a, E: nom::error::ParseError<&'a [u8]> + nom::error::ContextError<&'a [u8]>>(
        &self,
        buffer: &'a [u8],
    ) -> nom::IResult<&'a [u8], PeerId, E> {
        self.public_key_deserializer
            .deserialize(buffer)
            .map(|(buffer, public_key)| (buffer, PeerId { public_key }))
    }
}

impl ::serde::Serialize for PeerId {
    /// `::serde::Serialize` trait for `PeerId`
    ///
    fn serialize<S: ::serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.collect_str(&self.to_string())
    }
}

impl<'de> ::serde::Deserialize<'de> for PeerId {
    /// `::serde::Deserialize` trait for `PeerId`
    fn deserialize<D: ::serde::Deserializer<'de>>(d: D) -> Result<PeerId, D::Error> {
        struct Base58CheckVisitor;

        impl<'de> ::serde::de::Visitor<'de> for Base58CheckVisitor {
            type Value = PeerId;

            fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
                formatter.write_str("an ASCII base58check string")
            }

            fn visit_bytes<E>(self, v: &[u8]) -> Result<Self::Value, E>
            where
                E: ::serde::de::Error,
            {
                if let Ok(v_str) = std::str::from_utf8(v) {
                    PeerId::from_str(v_str).map_err(E::custom)
                } else {
                    Err(E::invalid_value(::serde::de::Unexpected::Bytes(v), &self))
                }
            }

            fn visit_str<E>(self, v: &str) -> Result<Self::Value, E>
            where
                E: ::serde::de::Error,
            {
                PeerId::from_str(v).map_err(E::custom)
            }
        }
        d.deserialize_str(Base58CheckVisitor)
    }
}
