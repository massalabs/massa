use std::fmt::Display;

use massa_hash::Hash;
use massa_serialization::{Serializer, Deserializer, SerializeError};
use massa_signature::{
    sign, verify_signature, PrivateKey, PublicKey, Signature,
};
use serde::{Deserialize, Serialize};
use nom::{IResult, error::{ContextError, ParseError}};

use crate::ModelsError;

/// Signed structure T where U is the associated id
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Signed<T, U>
where
    T: Display,
    U: Id,
{
    /// content
    pub content: T,
    /// signature
    pub signature: Signature,
    /// Id
    pub id: U,
    /// Content serialized
    pub serialized_data: Vec<u8>,
}

/// Used by signed structure
pub trait Id {
    /// new id from hash
    fn new(hash: Hash) -> Self;
}

impl<T, U> Display for Signed<T, U>
where
    T: Display,
{
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        writeln!(f, "Signature: {}", self.signature)?;
        writeln!(f, "{}", self.content)?;
        Ok(())
    }
}

impl<T, U> Signed<T, U>
where
    T: Display,
    U: Id,
{
    /// generate new signed structure and id
    pub fn new_signed(content: T, private_key: &PrivateKey) -> Result<Self, ModelsError> {
        let serialized_data = content.to_bytes_compact()?;
        let hash = Hash::compute_from(&serialized_data);
        Ok(Self {
            signature: sign(&hash, private_key)?,
            content,
            serialized_data,
            id: U::new(hash),
        })
    }

    /// check if self has been signed by public key
    pub fn verify_signature(&self, public_key: &PublicKey) -> Result<(), ModelsError> {
        Ok(verify_signature(
            &self.content.get_signature_message()?,
            &self.signature,
            public_key,
        )?)
    }
}

pub struct SignedSerializer<T, U>
where
    T: Display,
    U: Id;

impl<T, U> Serializer<Signed<T, U>> for SignedSerializer<T, U> {
    fn serialize(&self, value: &T) -> Result<Vec<u8>, SerializeError> {
        Ok(value.serialized_data)
    }
}

pub struct SignedDeserializer<T, U>
where
    T: Display,
    U: Id;

impl<T, U> Deserializer<Signed<T, U>> for SignedDeserializer<T, U> {
    fn deserialize<'a, E: ParseError<&'a [u8]> + ContextError<&'a [u8]>>(
            &self,
            buffer: &'a [u8],
        ) -> IResult<&'a [u8], Signed<T, U>, E> {
            Err(nom::Err::Error(ParseError::from_error_kind(
                buffer,
                nom::error::ErrorKind::Fail,
            )))
    }
}
