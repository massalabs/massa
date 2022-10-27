use std::fmt::Display;

use crate::{address::Address, error::ModelsError};
use massa_hash::Hash;
use massa_serialization::{Deserializer, SerializeError, Serializer};
use massa_signature::{
    KeyPair, PublicKey, PublicKeyDeserializer, Signature, SignatureDeserializer,
    PUBLIC_KEY_SIZE_BYTES, SIGNATURE_SIZE_BYTES,
};
use nom::{
    error::{context, ContextError, ParseError},
    sequence::tuple,
    IResult,
};
use serde::{Deserialize, Serialize};

pub trait Hasher {
    fn compute_from(&self, public_key: &[u8; 32], content: &[u8]) -> Option<Hash> {
        let mut hash_data = Vec::with_capacity(public_key.len() + content.len());
        hash_data.extend(public_key);
        hash_data.extend(content);
        Some(Hash::compute_from(&hash_data))
    }
}

/// Wrapped structure T where U is the associated id
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
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
    /// Id
    pub id: U,
    #[serde(skip)]
    /// Content serialized
    pub serialized_data: Vec<u8>,
}

/// Used by signed structure
pub trait Id {
    /// New id from hash
    fn new(hash: Hash) -> Self;
    /// Get a reference to the underlying hash
    fn get_hash(&self) -> &Hash;
}

/// Trait that define a structure that can be wrapped.
pub trait WrappedContent
where
    Self: Sized + Display + Hasher,
{
    /// Creates a wrapped version of the object
    fn new_wrapped<SC: Serializer<Self>, U: Id>(
        content: Self,
        content_serializer: SC,
        keypair: &KeyPair,
    ) -> Result<Wrapped<Self, U>, ModelsError> {
        let mut content_serialized = Vec::new();
        content_serializer.serialize(&content, &mut content_serialized)?;
        let public_key = keypair.get_public_key();
        let hash = content
            .compute_from(public_key.to_bytes(), &content_serialized)
            .ok_or(ModelsError::HashError)?;
        let creator_address = Address::from_public_key(&public_key);
        Ok(Wrapped {
            signature: keypair.sign(&hash)?,
            creator_public_key: public_key,
            creator_address,
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
        content_serializer: Option<&dyn Serializer<Self>>,
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
        let content_serialized = if let Some(content_serializer) = content_serializer {
            let mut content_buffer = Vec::new();
            content_serializer
                .serialize(&content, &mut content_buffer)
                .map_err(|_| {
                    nom::Err::Error(ParseError::from_error_kind(
                        rest,
                        nom::error::ErrorKind::Fail,
                    ))
                })?;
            content_buffer
        } else {
            // Avoid getting the rest of the data in the serialized data
            serialized_data[..serialized_data.len() - rest.len()].to_vec()
        };
        let creator_address = Address::from_public_key(&creator_public_key);
        let mut serialized_full_data = creator_public_key.to_bytes().to_vec();
        serialized_full_data.extend(&content_serialized);
        let hash = content
            .compute_from(creator_public_key.to_bytes(), &content_serialized)
            .ok_or(
                nom::Err::Error(ParseError::from_error_kind(
                    rest,
                    nom::error::ErrorKind::Fail,
                ))
            )?;
        Ok((
            rest,
            Wrapped {
                content,
                signature,
                creator_public_key,
                creator_address,
                serialized_data: content_serialized.to_vec(),
                id: U::new(hash),
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
        writeln!(f, "Id: {}", self.id.get_hash())?;
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
    pub fn verify_signature(&self) -> Result<(), ModelsError> {
        Ok(self
            .creator_public_key
            .verify_signature(self.id.get_hash(), &self.signature)?)
    }

    /// get full serialized size
    pub fn serialized_size(&self) -> usize {
        self.serialized_data
            .len()
            .saturating_add(SIGNATURE_SIZE_BYTES)
            .saturating_add(PUBLIC_KEY_SIZE_BYTES)
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

    /// This method is used to serialize a `Wrapped` structure and use a custom serializer instead of
    /// using the serialized form of the content stored in `serialized_data`.
    /// This is useful when the content need to be serialized in a lighter form in specific cases.
    ///
    /// # Arguments:
    /// * `serializer_content`: Custom serializer to be used instead of the data in `serialized_data`
    /// * `value`: Wrapped structure to be serialized
    /// * `buffer`: buffer of serialized data to be extend
    pub fn serialize_with<SC, T, U>(
        &self,
        serializer_content: &SC,
        value: &Wrapped<T, U>,
        buffer: &mut Vec<u8>,
    ) -> Result<(), SerializeError>
    where
        SC: Serializer<T>,
        T: Display + WrappedContent,
        U: Id,
    {
        let mut content_buffer = Vec::new();
        serializer_content.serialize(&value.content, &mut content_buffer)?;
        T::serialize(
            &value.signature,
            &value.creator_public_key,
            &content_buffer,
            buffer,
        )
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
    /// Creates a new `WrappedDeserializer`
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

    /// This method is used deserialize data that has been serialized in a lightweight form.
    /// The buffer doesn't have the whole content serialized and so
    /// this serialized data isn't coherent with the full structure and can't be used to calculate id and signature.
    /// We pass a serializer to serialize the full structure and retrieve a coherent `serialized_data`
    /// that can be use for the id and signature computing.
    ///
    /// # Arguments:
    /// * `content_serializer`: Serializer use to compute the `serialized_data` from the content
    /// * `buffer`: buffer of serialized data to be deserialized
    ///
    /// # Returns:
    /// A rest and the wrapped structure with coherent fields.
    pub fn deserialize_with<
        'a,
        E: ParseError<&'a [u8]> + ContextError<&'a [u8]>,
        U: Id,
        ST: Serializer<T>,
    >(
        &self,
        content_serializer: &ST,
        buffer: &'a [u8],
    ) -> IResult<&'a [u8], Wrapped<T, U>, E> {
        T::deserialize(
            Some(content_serializer),
            &self.signature_deserializer,
            &self.public_key_deserializer,
            &self.content_deserializer,
            buffer,
        )
    }
}

impl<T, U, DT> Deserializer<Wrapped<T, U>> for WrappedDeserializer<T, DT>
where
    T: Display + WrappedContent,
    U: Id,
    DT: Deserializer<T>,
{
    /// ```
    /// # use massa_models::{block::BlockId, endorsement::{Endorsement, EndorsementSerializer, EndorsementDeserializer}, slot::Slot, wrapped::{Wrapped, WrappedSerializer, WrappedDeserializer, WrappedContent}};
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
    /// let deserializer = WrappedDeserializer::new(EndorsementDeserializer::new(32, 1));
    /// let (rest, deserialized): (&[u8], Wrapped<Endorsement, BlockId>) = deserializer.deserialize::<DeserializeError>(&serialized_data).unwrap();
    /// assert!(rest.is_empty());
    /// assert_eq!(wrapped.id, deserialized.id);
    /// ```
    fn deserialize<'a, E: ParseError<&'a [u8]> + ContextError<&'a [u8]>>(
        &self,
        buffer: &'a [u8],
    ) -> IResult<&'a [u8], Wrapped<T, U>, E> {
        T::deserialize(
            None,
            &self.signature_deserializer,
            &self.public_key_deserializer,
            &self.content_deserializer,
            buffer,
        )
    }
}
