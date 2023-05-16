use massa_signature::{PublicKey, KeyPair, PublicKeyDeserializer};
use peernet::peer_id::PeerId as PeernetPeerId;
use massa_serialization::{Serializer, DeserializeError, Deserializer};

#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct PeerId {
    public_key: PublicKey,
}

impl PeerId {
    pub fn from_public_key(public_key: PublicKey) -> Self {
        Self { public_key }
    }
}

impl PeernetPeerId for PeerId {
    fn generate() -> Self {
        Self { public_key: KeyPair::generate(0).unwrap().get_public_key() }
    }
}

pub struct PeerIdSerializer {}

impl Serializer<PeerId> for PeerIdSerializer {
    fn serialize(&self, value: &PeerId, buffer: &mut Vec<u8>) -> Result<(), massa_serialization::SerializeError> {
        buffer.extend_from_slice(&value.public_key.to_bytes());
        Ok(())
    }
}

pub struct PeerIdDeserializer {
    public_key_deserializer: PublicKeyDeserializer,
}

impl Deserializer<PeerId> for PeerIdDeserializer {
    fn deserialize<'a, E: nom::error::ParseError<&'a [u8]> + nom::error::ContextError<&'a [u8]>>(
            &self,
            buffer: &'a [u8],
        ) -> nom::IResult<&'a [u8], PeerId, E> {
        self.public_key_deserializer.deserialize(buffer).map(|(buffer, public_key)| {
            (buffer, PeerId { public_key })
        })
    }
}