use std::fmt::Display;

use massa_hash::Hash;
use massa_serialization::{Deserializer, SerializeError, Serializer};
use massa_signature::{
    derive_public_key, sign, verify_signature, PrivateKey, PublicKey, PublicKeyDeserializer,
    Signature, SignatureDeserializer,
};
use nom::{
    error::{ContextError, ParseError},
    sequence::tuple,
    IResult,
};
use serde::{Deserialize, Serialize};

use crate::{node_configuration::THREAD_COUNT, Address, ModelsError};

/// Wrapped structure T where U is the associated id
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Wrapped<T, U>
where
    T: Display,
    U: Id,
{
    /// content
    pub content: T,
    /// signature
    pub signature: Signature,
    /// the content creator public key
    pub creator_public_key: PublicKey,
    /// the content creator address
    pub creator_address: Address,
    /// Thread of the operation creator
    pub thread: u8,
    /// Id
    pub id: U,
    /// Content serialized
    pub serialized_data: Vec<u8>,
}

/// Used by signed structure
pub trait Id {
    /// new id from hash
    fn new(hash: Hash) -> Self;
    /// Get the hash
    fn hash(&self) -> Hash;
}

impl<T, U> Display for Wrapped<T, U>
where
    T: Display,
    U: Id,
{
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        writeln!(f, "Signature: {}", self.signature)?;
        writeln!(f, "Creator pubkey: {}", self.creator_public_key)?;
        writeln!(f, "Creator address: {}", self.creator_address)?;
        writeln!(f, "Id: {}", self.id.hash())?;
        writeln!(f, "{}", self.content)?;
        Ok(())
    }
}

impl<T, U> Wrapped<T, U>
where
    T: Display,
    U: Id,
{
    /// generate new signed structure and id
    pub fn new_wrapped<ST: Serializer<T>>(
        content: T,
        content_serializer: ST,
        private_key: &PrivateKey,
    ) -> Result<Self, ModelsError> {
        let mut serialized_data = Vec::new();
        let creator_public_key = derive_public_key(private_key);
        serialized_data.extend(creator_public_key.to_bytes());
        content_serializer.serialize(&content, &mut serialized_data)?;
        let hash = Hash::compute_from(&serialized_data);
        let creator_address = Address::from_public_key(&creator_public_key);
        #[cfg(feature = "sandbox")]
        let thread_count = *THREAD_COUNT;
        #[cfg(not(feature = "sandbox"))]
        let thread_count = THREAD_COUNT;
        Ok(Self {
            signature: sign(&hash, private_key)?,
            creator_public_key,
            creator_address,
            thread: creator_address.get_thread(thread_count),
            content,
            serialized_data,
            id: U::new(hash),
        })
    }

    /// check if self has been signed by public key
    pub fn verify_signature(&self, public_key: &PublicKey) -> Result<(), ModelsError> {
        Ok(verify_signature(
            &self.id.hash(),
            &self.signature,
            public_key,
        )?)
    }
}

// NOTE FOR EXPLICATION: No content serializer because serialized data is already here.
/// Serializer for `Wrapped` structure
pub struct WrappedSerializer<T, U>
where
    T: Display,
    U: Id,
{
    marker_t: std::marker::PhantomData<T>,
    marker_u: std::marker::PhantomData<U>,
}

impl<T, U> WrappedSerializer<T, U>
where
    T: Display,
    U: Id,
{
    /// Creates a new `WrappedSerializer`
    pub fn new() -> Self {
        Self {
            marker_t: std::marker::PhantomData,
            marker_u: std::marker::PhantomData,
        }
    }
}

impl<T, U> Serializer<Wrapped<T, U>> for WrappedSerializer<T, U>
where
    T: Display,
    U: Id,
{
    fn serialize(&self, value: &Wrapped<T, U>, buffer: &mut Vec<u8>) -> Result<(), SerializeError> {
        buffer.extend(value.signature.into_bytes());
        buffer.extend(value.serialized_data.clone());
        Ok(())
    }
}

/// Deserializer for Wrapped structure
pub struct WrappedDeserializer<T, U, DT>
where
    T: Display,
    U: Id,
    DT: Deserializer<T>,
{
    signature_deserializer: SignatureDeserializer,
    public_key_deserializer: PublicKeyDeserializer,
    content_deserializer: DT,
    marker_t: std::marker::PhantomData<T>,
    marker_u: std::marker::PhantomData<U>,
}

impl<T, U, DT> WrappedDeserializer<T, U, DT>
where
    T: Display,
    U: Id,
    DT: Deserializer<T>,
{
    /// Creates a new WrappedDeserializer
    ///
    /// # Arguments
    /// * `content_deserializer` - Deserializer for the content
    pub fn new(content_deserializer: DT) -> Self {
        Self {
            signature_deserializer: SignatureDeserializer::new(),
            public_key_deserializer: PublicKeyDeserializer::new(),
            content_deserializer,
            marker_t: std::marker::PhantomData,
            marker_u: std::marker::PhantomData,
        }
    }
}

impl<T, U, DT> Deserializer<Wrapped<T, U>> for WrappedDeserializer<T, U, DT>
where
    T: Display,
    U: Id,
    DT: Deserializer<T>,
{
    fn deserialize<'a, E: ParseError<&'a [u8]> + ContextError<&'a [u8]>>(
        &self,
        buffer: &'a [u8],
    ) -> IResult<&'a [u8], Wrapped<T, U>, E> {
        let (serialized_data, (signature, creator_public_key)) = tuple((
            |input| self.signature_deserializer.deserialize(input),
            |input| self.public_key_deserializer.deserialize(input),
        ))(buffer)?;
        #[cfg(feature = "sandbox")]
        let thread_count = *THREAD_COUNT;
        #[cfg(not(feature = "sandbox"))]
        let thread_count = THREAD_COUNT;
        let (rest, content) = self.content_deserializer.deserialize(serialized_data)?;
        // Avoid getting the rest of the data in the serialized data
        let serialized_data = &serialized_data[..serialized_data.len() - rest.len()];
        let creator_address = Address::from_public_key(&creator_public_key);
        Ok((
            rest,
            Wrapped {
                content,
                signature,
                creator_public_key,
                creator_address,
                thread: creator_address.get_thread(thread_count),
                serialized_data: serialized_data.to_vec(),
                id: U::new(Hash::compute_from(serialized_data)),
            },
        ))
    }
}
