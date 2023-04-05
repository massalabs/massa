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
use crate::address::ADDRESS_SIZE_BYTES;

/// Packages type T such that it can be securely sent and received in a trust-free network
///
/// If the internal content is mutated, then it must be re-wrapped, as the assosciated
/// signature, serialized data, etc. would no longer be in sync
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SecureShare<T, ID>
where
    T: Display + SecureShareContent,
    ID: Id,
{
    /// Reference contents. Not required for the the security protocols.
    ///
    /// Use the Lightweight equivilant structures when you need verifiable
    /// serialized data, but do not need to read the values directly (such as when sending)
    pub content: T,
    #[serde(skip)]
    /// Content in sharable, deserializable form. Is used in the secure verification protocols.
    pub serialized_data: Vec<u8>,

    /// A cryptographically generated value using `serialized_data` and a public key.
    pub signature: Signature,
    /// The public-key component used in the generation of the signature
    pub content_creator_pub_key: PublicKey,
    /// Derived from the same public key used to generate the signature
    pub content_creator_address: Address,
    /// A secure hash of the data. See also [massa_hash::Hash]
    pub id: ID,
}

/// Used by signed structure
pub trait Id {
    /// New id from hash
    fn new(hash: Hash) -> Self;
    /// Get a reference to the underlying hash
    fn get_hash(&self) -> &Hash;
}

/// Trait that define a structure that can be signed for secure sharing.
pub trait SecureShareContent
where
    Self: Sized + Display,
{
    /// Using the provided key-pair, applies a cryptographic signature, and packages
    /// the data required to share and verify the data in a trust-free network of peers.
    fn new_verifiable<Ser: Serializer<Self>, ID: Id>(
        content: Self,
        content_serializer: Ser,
        keypair: &KeyPair,
    ) -> Result<SecureShare<Self, ID>, ModelsError> {
        let mut content_serialized = Vec::new();
        content_serializer.serialize(&content, &mut content_serialized)?;
        let public_key = keypair.get_public_key();
        let hash = Self::compute_hash(&content, &content_serialized, &public_key);
        let creator_address = Address::from_public_key(&public_key);
        Ok(SecureShare {
            signature: keypair.sign(&hash)?,
            content_creator_pub_key: public_key,
            content_creator_address: creator_address,
            content,
            serialized_data: content_serialized,
            id: ID::new(hash),
        })
    }

    /// Compute hash
    #[allow(unused_variables)]
    fn compute_hash(
        content: &Self, // use only for Denounce able object
        content_serialized: &[u8],
        content_creator_pub_key: &PublicKey,
    ) -> Hash {
        let mut hash_data = Vec::new();
        hash_data.extend(content_creator_pub_key.to_bytes());
        hash_data.extend(content_serialized);
        Hash::compute_from(&hash_data)
    }

    /// Serialize the secured structure
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

    /// Deserialize the secured structure
    fn deserialize<
        'a,
        E: ParseError<&'a [u8]> + ContextError<&'a [u8]>,
        Deser: Deserializer<Self>,
        ID: Id,
    >(
        content_serializer: Option<&dyn Serializer<Self>>,
        signature_deserializer: &SignatureDeserializer,
        creator_public_key_deserializer: &PublicKeyDeserializer,
        content_deserializer: &Deser,
        buffer: &'a [u8],
    ) -> IResult<&'a [u8], SecureShare<Self, ID>, E> {
        let (serialized_data, (signature, creator_public_key)) = context(
            "Failed SecureShare deserialization",
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
        let hash = Self::compute_hash(&content, &content_serialized, &creator_public_key);

        Ok((
            rest,
            SecureShare {
                content,
                signature,
                content_creator_pub_key: creator_public_key,
                content_creator_address: creator_address,
                serialized_data: content_serialized.to_vec(),
                id: ID::new(hash),
            },
        ))
    }
}

impl<T, ID> Display for SecureShare<T, ID>
where
    T: Display + SecureShareContent,
    ID: Id,
{
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        writeln!(f, "Signature: {}", self.signature)?;
        writeln!(f, "Creator pubkey: {}", self.content_creator_pub_key)?;
        writeln!(f, "Creator address: {}", self.content_creator_address)?;
        writeln!(f, "Id: {}", self.id.get_hash())?;
        writeln!(f, "{}", self.content)?;
        Ok(())
    }
}

impl<T, ID> SecureShare<T, ID>
where
    T: Display + SecureShareContent,
    ID: Id,
{
    /// check if self has been signed by public key
    pub fn verify_signature(&self) -> Result<(), ModelsError> {
        Ok(self
            .content_creator_pub_key
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
/// Serializer for `SecureShare` structure
#[derive(Default)]
pub struct SecureShareSerializer;

impl SecureShareSerializer {
    /// Creates a new `SecureShareSerializer`
    pub const fn new() -> Self {
        Self
    }

    /// This method is used to serialize a `SecureShare` structure and use a custom serializer instead of
    /// using the serialized form of the content stored in `serialized_data`.
    /// This is useful when the content need to be serialized in a lighter form in specific cases.
    ///
    /// # Arguments:
    /// * `content_serializer`: Custom serializer to be used instead of the data in `serialized_data`
    /// * `value`: SecureShare structure to be serialized
    /// * `buffer`: buffer of serialized data to be extend
    pub fn serialize_with<Ser, T, ID>(
        &self,
        content_serializer: &Ser,
        value: &SecureShare<T, ID>,
        buffer: &mut Vec<u8>,
    ) -> Result<(), SerializeError>
    where
        Ser: Serializer<T>,
        T: Display + SecureShareContent,
        ID: Id,
    {
        let mut content_buffer = Vec::new();
        content_serializer.serialize(&value.content, &mut content_buffer)?;
        T::serialize(
            &value.signature,
            &value.content_creator_pub_key,
            &content_buffer,
            buffer,
        )
    }
}

impl<T, ID> Serializer<SecureShare<T, ID>> for SecureShareSerializer
where
    T: Display + SecureShareContent,
    ID: Id,
{
    fn serialize(
        &self,
        value: &SecureShare<T, ID>,
        buffer: &mut Vec<u8>,
    ) -> Result<(), SerializeError> {
        T::serialize(
            &value.signature,
            &value.content_creator_pub_key,
            &value.serialized_data,
            buffer,
        )
    }
}

/// Deserializer for SecureShare structure
pub struct SecureShareDeserializer<T, Deser>
where
    T: Display + SecureShareContent,
    Deser: Deserializer<T>,
{
    signature_deserializer: SignatureDeserializer,
    public_key_deserializer: PublicKeyDeserializer,
    content_deserializer: Deser,
    marker_t: std::marker::PhantomData<T>,
}

impl<T, Deser> SecureShareDeserializer<T, Deser>
where
    T: Display + SecureShareContent,
    Deser: Deserializer<T>,
{
    /// Creates a new `SecureShareDeserializer`
    ///
    /// # Arguments
    /// * `content_deserializer` - Deserializer for the content
    pub const fn new(content_deserializer: Deser) -> Self {
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
    /// A rest (data left over from deserialization), an instance of `T`, and the data enabling signature verification
    pub fn deserialize_with<
        'a,
        E: ParseError<&'a [u8]> + ContextError<&'a [u8]>,
        ID: Id,
        Ser: Serializer<T>,
    >(
        &self,
        content_serializer: &Ser,
        buffer: &'a [u8],
    ) -> IResult<&'a [u8], SecureShare<T, ID>, E> {
        T::deserialize(
            Some(content_serializer),
            &self.signature_deserializer,
            &self.public_key_deserializer,
            &self.content_deserializer,
            buffer,
        )
    }
}

impl<T, ID, Deser> Deserializer<SecureShare<T, ID>> for SecureShareDeserializer<T, Deser>
where
    T: Display + SecureShareContent,
    ID: Id,
    Deser: Deserializer<T>,
{
    /// ```
    /// # use massa_models::{endorsement::{Endorsement, EndorsementSerializer, EndorsementDeserializer}, slot::Slot, secure_share::{SecureShare, SecureShareSerializer, SecureShareDeserializer, SecureShareContent}};
    /// use massa_models::block_id::BlockId;
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
    /// let secured: SecureShare<Endorsement, BlockId> = Endorsement::new_verifiable(
    ///    content,
    ///    EndorsementSerializer::new(),
    ///    &keypair
    /// ).unwrap();
    /// let mut serialized_data = Vec::new();
    /// let serialized = SecureShareSerializer::new().serialize(&secured, &mut serialized_data).unwrap();
    /// let deserializer = SecureShareDeserializer::new(EndorsementDeserializer::new(32, 1));
    /// let (rest, deserialized): (&[u8], SecureShare<Endorsement, BlockId>) = deserializer.deserialize::<DeserializeError>(&serialized_data).unwrap();
    /// assert!(rest.is_empty());
    /// assert_eq!(secured.id, deserialized.id);
    /// ```
    fn deserialize<'a, E: ParseError<&'a [u8]> + ContextError<&'a [u8]>>(
        &self,
        buffer: &'a [u8],
    ) -> IResult<&'a [u8], SecureShare<T, ID>, E> {
        T::deserialize(
            None,
            &self.signature_deserializer,
            &self.public_key_deserializer,
            &self.content_deserializer,
            buffer,
        )
    }
}
