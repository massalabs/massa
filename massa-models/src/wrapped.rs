use std::fmt::Display;

use crate::{node_configuration::THREAD_COUNT, Address, ModelsError};
use massa_hash::Hash;
use massa_serialization::{Deserializer, SerializeError, Serializer};
use massa_signature::{
    KeyPair, PublicKey, PublicKeyDeserializer, Signature, SignatureDeserializer,
};
use nom::{
    error::{context, ContextError, ParseError},
    sequence::tuple,
    IResult,
};
use serde::{Deserialize, Serialize};

/// Wrapped structure T where U is the associated id
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Wrapped<T, U>
where
    T: Display + WrappedContent,
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
    #[serde(skip)]
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

/// Trait that define a structure that can be wrapped.
pub trait WrappedContent
where
    Self: Sized + Display,
{
    /// Creates a wrapped version of the object
    fn new_wrapped<SC: Serializer<Self>, U: Id>(
        content: Self,
        content_serializer: SC,
        keypair: &KeyPair,
    ) -> Result<Wrapped<Self, U>, ModelsError> {
        let mut content_serialized = Vec::new();
        content_serializer.serialize(&content, &mut content_serialized)?;
        let mut hash_data = Vec::new();
        let public_key = keypair.get_public_key();
        hash_data.extend(public_key.to_bytes());
        hash_data.extend(content_serialized.clone());
        let hash = Hash::compute_from(&hash_data);
        let creator_address = Address::from_public_key(&public_key);
        Ok(Wrapped {
            signature: keypair.sign(&hash)?,
            creator_public_key: public_key,
            creator_address,
            thread: creator_address.get_thread(THREAD_COUNT),
            content,
            serialized_data: content_serialized,
            id: U::new(hash),
        })
    }

    /// Serialize the wrapped structure
    fn serialize(
        signature: &Signature,
        creator_public_key: &PublicKey,
        serialized_content: &[u8],
        buffer: &mut Vec<u8>,
    ) -> Result<(), SerializeError> {
        buffer.extend(signature.into_bytes());
        buffer.extend(creator_public_key.into_bytes());
        buffer.extend(serialized_content);
        Ok(())
    }

    /// Deserialize the wrapped structure
    fn deserialize<
        'a,
        E: ParseError<&'a [u8]> + ContextError<&'a [u8]>,
        DC: Deserializer<Self>,
        U: Id,
    >(
        signature_deserializer: &SignatureDeserializer,
        creator_public_key_deserializer: &PublicKeyDeserializer,
        content_deserializer: &DC,
        buffer: &'a [u8],
    ) -> IResult<&'a [u8], Wrapped<Self, U>, E> {
        let (serialized_data, (signature, creator_public_key)) = context(
            "Failed wrapped deserialization",
            tuple((
                context("Failed signature deserialization", |input| {
                    signature_deserializer.deserialize(input)
                }),
                context("Failed public_key deserialization", |input| {
                    creator_public_key_deserializer.deserialize(input)
                }),
            )),
        )(buffer)?;
        let (rest, content) = content_deserializer.deserialize(serialized_data)?;
        // Avoid getting the rest of the data in the serialized data
        let content_serialized = &serialized_data[..serialized_data.len() - rest.len()];
        let creator_address = Address::from_public_key(&creator_public_key);
        let mut serialized_full_data = creator_public_key.to_bytes().to_vec();
        serialized_full_data.extend(content_serialized);
        Ok((
            rest,
            Wrapped {
                content,
                signature,
                creator_public_key,
                creator_address,
                thread: creator_address.get_thread(THREAD_COUNT),
                serialized_data: content_serialized.to_vec(),
                id: U::new(Hash::compute_from(&serialized_full_data)),
            },
        ))
    }
}

impl<T, U> Display for Wrapped<T, U>
where
    T: Display + WrappedContent,
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
    T: Display + WrappedContent,
    U: Id,
{
    /// check if self has been signed by public key
    pub fn verify_signature<SC: Serializer<T>>(
        &self,
        content_serializer: SC,
        public_key: &PublicKey,
    ) -> Result<(), ModelsError> {
        let mut content_serialized = Vec::new();
        content_serializer.serialize(&self.content, &mut content_serialized)?;
        let mut hash_data = Vec::new();
        hash_data.extend(self.creator_public_key.to_bytes());
        hash_data.extend(content_serialized.clone());
        let hash = Hash::compute_from(&hash_data);
        Ok(public_key.verify_signature(&hash, &self.signature)?)
    }
}

// NOTE FOR EXPLICATION: No content serializer because serialized data is already here.
/// Serializer for `Wrapped` structure
#[derive(Default)]
pub struct WrappedSerializer;

impl WrappedSerializer {
    /// Creates a new `WrappedSerializer`
    pub const fn new() -> Self {
        Self
    }
}

impl<T, U> Serializer<Wrapped<T, U>> for WrappedSerializer
where
    T: Display + WrappedContent,
    U: Id,
{
    fn serialize(&self, value: &Wrapped<T, U>, buffer: &mut Vec<u8>) -> Result<(), SerializeError> {
        T::serialize(
            &value.signature,
            &value.creator_public_key,
            &value.serialized_data,
            buffer,
        )
    }
}

/// Deserializer for Wrapped structure
pub struct WrappedDeserializer<T, DT>
where
    T: Display + WrappedContent,
    DT: Deserializer<T>,
{
    signature_deserializer: SignatureDeserializer,
    public_key_deserializer: PublicKeyDeserializer,
    content_deserializer: DT,
    marker_t: std::marker::PhantomData<T>,
}

impl<T, DT> WrappedDeserializer<T, DT>
where
    T: Display + WrappedContent,
    DT: Deserializer<T>,
{
    /// Creates a new WrappedDeserializer
    ///
    /// # Arguments
    /// * `content_deserializer` - Deserializer for the content
    pub const fn new(content_deserializer: DT) -> Self {
        Self {
            signature_deserializer: SignatureDeserializer::new(),
            public_key_deserializer: PublicKeyDeserializer::new(),
            content_deserializer,
            marker_t: std::marker::PhantomData,
        }
    }
}

impl<T, U, DT> Deserializer<Wrapped<T, U>> for WrappedDeserializer<T, DT>
where
    T: Display + WrappedContent,
    U: Id,
    DT: Deserializer<T>,
{
    /// ```
    /// # use massa_models::{BlockId, Endorsement, EndorsementSerializer, EndorsementDeserializer, Slot, wrapped::{Wrapped, WrappedSerializer, WrappedDeserializer, WrappedContent}};
    /// # use massa_serialization::{Deserializer, Serializer, DeserializeError, U16VarIntSerializer, U16VarIntDeserializer};
    /// # use massa_signature::KeyPair;
    /// # use std::ops::Bound::Included;
    /// # use massa_hash::Hash;
    ///
    /// let content = Endorsement {
    ///    slot: Slot::new(10, 1),
    ///    index: 0,
    ///    endorsed_block: BlockId(Hash::compute_from("blk".as_bytes())),
    /// };
    /// let keypair = KeyPair::generate();
    /// let wrapped: Wrapped<Endorsement, BlockId> = Endorsement::new_wrapped(
    ///    content,
    ///    EndorsementSerializer::new(),
    ///    &keypair
    /// ).unwrap();
    /// let mut serialized_data = Vec::new();
    /// let serialized = WrappedSerializer::new().serialize(&wrapped, &mut serialized_data).unwrap();
    /// let deserializer = WrappedDeserializer::new(EndorsementDeserializer::new(1));
    /// let (rest, deserialized): (&[u8], Wrapped<Endorsement, BlockId>) = deserializer.deserialize::<DeserializeError>(&serialized_data).unwrap();
    /// assert!(rest.is_empty());
    /// assert_eq!(wrapped.id, deserialized.id);
    /// ```
    fn deserialize<'a, E: ParseError<&'a [u8]> + ContextError<&'a [u8]>>(
        &self,
        buffer: &'a [u8],
    ) -> IResult<&'a [u8], Wrapped<T, U>, E> {
        T::deserialize(
            &self.signature_deserializer,
            &self.public_key_deserializer,
            &self.content_deserializer,
            buffer,
        )
    }
}
