// Copyright (c) 2022 MASSA LABS <info@massa.net>

use crate::error::MassaSignatureError;

use ed25519_dalek::{Signer, Verifier};

use massa_hash::Hash;
use massa_serialization::{
    DeserializeError, Deserializer, Serializer, U64VarIntDeserializer, U64VarIntSerializer,
};
use nom::{
    error::{ContextError, ParseError},
    IResult,
};
use rand::rngs::OsRng;
use serde::{
    de::{MapAccess, SeqAccess, Visitor},
    ser::SerializeStruct,
    Deserialize,
};
use std::{borrow::Cow, cmp::Ordering, hash::Hasher, ops::Bound::Included};
use std::{convert::TryInto, str::FromStr};

use transition::Versioned;

// LEGACY TODO (not sure what this meant): REMOVE THESE DECLARATIONS AND HANDLE ERRORS

/// Size in bytes of a public key
pub const PUBLIC_KEY_SIZE_BYTES: usize = 32;
/// Size in bytes of signature key
pub const SIGNATURE_SIZE_BYTES: usize = 32;

#[allow(missing_docs)]
/// A versioned KeyPair used for signature and decryption
#[transition::versioned(versions("0", "1"))]
pub struct KeyPair(ed25519_dalek::Keypair);

#[transition::impl_version(versions("0", "1"), structures("KeyPair"))]
impl KeyPair {
    /// Size of a keypair
    pub const SECRET_KEY_BYTES_SIZE: usize = ed25519_dalek::SECRET_KEY_LENGTH;
}

impl Clone for KeyPair {
    fn clone(&self) -> Self {
        //CallVersions!(self, fmt(f));
        //TODO: https://stackoverflow.com/questions/75171139/use-macro-in-match-branch
        match self {
            KeyPair::KeyPairV0(keypair) => KeyPair::KeyPairV0(keypair.clone()),
            KeyPair::KeyPairV1(keypair) => KeyPair::KeyPairV1(keypair.clone()),
        }
    }
}

const SECRET_PREFIX: char = 'S';

impl std::fmt::Display for KeyPair {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        match self {
            KeyPair::KeyPairV0(keypair) => keypair.fmt(f),
            KeyPair::KeyPairV1(keypair) => keypair.fmt(f),
        }
    }
}

impl std::fmt::Debug for KeyPair {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(f, "{}", self)
    }
}

impl FromStr for KeyPair {
    type Err = MassaSignatureError;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let mut chars = s.chars();
        match chars.next() {
            Some(prefix) if prefix == SECRET_PREFIX => {
                let data = chars.collect::<String>();
                let decoded_bs58_check =
                    bs58::decode(data)
                        .with_check(None)
                        .into_vec()
                        .map_err(|_| {
                            MassaSignatureError::ParsingError(format!("bad secret key bs58: {}", s))
                        })?;
                let u64_deserializer = U64VarIntDeserializer::new(Included(0), Included(u64::MAX));
                let (rest, version) = u64_deserializer
                    .deserialize::<DeserializeError>(&decoded_bs58_check[..])
                    .map_err(|err| MassaSignatureError::ParsingError(err.to_string()))?;
                match version {
                    <KeyPair!["0"]>::VERSION => Ok(KeyPairVariant!["0"](
                        <KeyPair!["0"]>::from_bytes(rest.try_into().map_err(|_| {
                            MassaSignatureError::ParsingError(format!(
                                "secret key not long enough for: {}",
                                s
                            ))
                        })?)?,
                    )),
                    <KeyPair!["1"]>::VERSION => Ok(KeyPairVariant!["1"](
                        <KeyPair!["1"]>::from_bytes(rest.try_into().map_err(|_| {
                            MassaSignatureError::ParsingError(format!(
                                "secret key not long enough for: {}",
                                s
                            ))
                        })?)?,
                    )),
                    _ => Err(MassaSignatureError::InvalidVersionError(format!(
                        "Unknown keypair version: {}",
                        version
                    ))),
                }
            }
            _ => Err(MassaSignatureError::ParsingError(format!(
                "bad secret prefix for: {}",
                s
            ))),
        }
    }
}

#[transition::impl_version(versions("0", "1"), structures("KeyPair"))]
impl KeyPair {
    pub fn get_version(&self) -> u64 {
        Self::VERSION
    }
}

impl KeyPair {
    /// Get the version of the given KeyPair
    pub fn get_version(&self) -> u64 {
        match self {
            KeyPair::KeyPairV0(keypair) => keypair.get_version(),
            KeyPair::KeyPairV1(keypair) => keypair.get_version(),
        }
    }

    /// Generates a new KeyPair of the version given as parameter.
    /// Errors if the version number does not exist
    ///
    /// # Example
    ///  ```
    /// # use massa_signature::KeyPair;
    /// # use massa_hash::Hash;
    /// let keypair = KeyPair::generate(0).unwrap();
    /// let data = Hash::compute_from("Hello World!".as_bytes());
    /// let signature = keypair.sign(&data).unwrap();
    ///
    /// let serialized: String = signature.to_bs58_check();
    pub fn generate(version: u64) -> Result<Self, MassaSignatureError> {
        match version {
            <KeyPair!["0"]>::VERSION => Ok(KeyPairVariant!["0"](<KeyPair!["0"]>::generate())),
            <KeyPair!["1"]>::VERSION => Ok(KeyPairVariant!["1"](<KeyPair!["1"]>::generate())),
            _ => Err(MassaSignatureError::InvalidVersionError(format!(
                "KeyPair version {} doesn't exist.",
                version
            ))),
        }
    }

    /// Returns the Signature produced by signing
    /// data bytes with a `KeyPair`.
    ///
    /// # Example
    ///  ```
    /// # use massa_signature::KeyPair;
    /// # use massa_hash::Hash;
    /// let keypair = KeyPair::generate(0).unwrap();
    /// let data = Hash::compute_from("Hello World!".as_bytes());
    /// let signature = keypair.sign(&data).unwrap();
    /// ```
    pub fn sign(&self, hash: &Hash) -> Result<Signature, MassaSignatureError> {
        match self {
            KeyPair::KeyPairV0(keypair) => keypair.sign(hash).map(Signature::SignatureV0),
            KeyPair::KeyPairV1(keypair) => keypair.sign(hash).map(Signature::SignatureV1),
        }
    }

    /// Return the bytes (as a Vec) representing the keypair
    ///
    /// # Example
    /// ```
    /// # use massa_signature::KeyPair;
    /// let keypair = KeyPair::generate(0).unwrap();
    /// let bytes = keypair.to_bytes();
    /// ```
    pub fn to_bytes(&self) -> Vec<u8> {
        match self {
            KeyPair::KeyPairV0(keypair) => keypair.to_bytes(),
            KeyPair::KeyPairV1(keypair) => keypair.to_bytes(),
        }
    }

    /// Get the public key of the keypair
    ///
    /// # Example
    /// ```
    /// # use massa_signature::KeyPair;
    /// let keypair = KeyPair::generate(0).unwrap();
    /// let public_key = keypair.get_public_key();
    /// ```
    pub fn get_public_key(&self) -> PublicKey {
        match self {
            KeyPair::KeyPairV0(keypair) => PublicKey::PublicKeyV0(keypair.get_public_key()),
            KeyPair::KeyPairV1(keypair) => PublicKey::PublicKeyV1(keypair.get_public_key()),
        }
    }

    /// Convert a byte slice to a `KeyPair`
    ///
    /// # Example
    /// ```
    /// # use massa_signature::KeyPair;
    /// let keypair = KeyPair::generate(0).unwrap();
    /// let bytes = keypair.into_bytes();
    /// let keypair2 = KeyPair::from_bytes(&bytes).unwrap();
    /// ```
    pub fn from_bytes(data: &[u8]) -> Result<Self, MassaSignatureError> {
        let u64_deserializer = U64VarIntDeserializer::new(Included(0), Included(u64::MAX));
        let (rest, version) = u64_deserializer
            .deserialize::<DeserializeError>(data)
            .map_err(|err| MassaSignatureError::ParsingError(err.to_string()))?;
        match version {
            <KeyPair!["0"]>::VERSION => Ok(KeyPairVariant!["0"](<KeyPair!["0"]>::from_bytes(
                rest.try_into().map_err(|err| {
                    MassaSignatureError::ParsingError(format!(
                        "keypair bytes parsing error: {}",
                        err
                    ))
                })?,
            )?)),
            <KeyPair!["1"]>::VERSION => Ok(KeyPairVariant!["1"](<KeyPair!["1"]>::from_bytes(
                rest.try_into().map_err(|err| {
                    MassaSignatureError::ParsingError(format!(
                        "keypair bytes parsing error: {}",
                        err
                    ))
                })?,
            )?)),
            _ => Err(MassaSignatureError::InvalidVersionError(format!(
                "Unknown keypair version: {}",
                version
            ))),
        }
    }
}

#[transition::impl_version(versions("0", "1"))]
impl Clone for KeyPair {
    fn clone(&self) -> Self {
        KeyPair(ed25519_dalek::Keypair {
            // This will never error since self is a valid keypair
            secret: ed25519_dalek::SecretKey::from_bytes(self.0.secret.as_bytes()).unwrap(),
            public: self.0.public,
        })
    }
}

#[transition::impl_version(versions("0", "1"))]
impl std::fmt::Display for KeyPair {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        let u64_serializer = U64VarIntSerializer::new();
        let mut bytes = Vec::new();
        u64_serializer
            .serialize(&Self::VERSION, &mut bytes)
            .map_err(|_| std::fmt::Error)?;
        bytes.extend(self.to_bytes());
        write!(
            f,
            "{}{}",
            SECRET_PREFIX,
            bs58::encode(bytes).with_check().into_string()
        )
    }
}

#[transition::impl_version(versions("0", "1"), structures("KeyPair", "Signature", "PublicKey"))]
impl KeyPair {
    /// Returns the Signature produced by signing
    /// data bytes with a `KeyPair`.
    ///
    /// # Example
    ///  ```
    /// # use massa_signature::KeyPair;
    /// # use massa_hash::Hash;
    /// let keypair = KeyPair::generate(0).unwrap();
    /// let data = Hash::compute_from("Hello World!".as_bytes());
    /// let signature = keypair.sign(&data).unwrap();
    /// ```
    pub fn sign(&self, hash: &Hash) -> Result<Signature, MassaSignatureError> {
        Ok(Signature(self.0.sign(hash.to_bytes())))
    }

    /// Get the public key of the keypair
    ///
    /// # Example
    /// ```
    /// # use massa_signature::KeyPair;
    /// let keypair = KeyPair::generate(0).unwrap();
    /// let public_key = keypair.get_public_key();
    /// ```
    pub fn get_public_key(&self) -> PublicKey {
        PublicKey(self.0.public)
    }

    /// Generate a new `KeyPair`
    ///
    /// # Example
    ///  ```
    /// # use massa_signature::KeyPair;
    /// # use massa_hash::Hash;
    /// let keypair = KeyPair::generate(0).unwrap();
    /// let data = Hash::compute_from("Hello World!".as_bytes());
    /// let signature = keypair.sign(&data).unwrap();
    ///
    /// let serialized: String = signature.to_bs58_check();
    /// ```
    pub fn generate() -> Self {
        let mut rng = OsRng::default();
        KeyPair(ed25519_dalek::Keypair::generate(&mut rng))
    }

    /// Convert a byte array of size `SECRET_KEY_BYTES_SIZE` to a `KeyPair`
    ///
    /// # Example
    /// ```
    /// # use massa_signature::KeyPair;
    /// let keypair = KeyPair::generate(0).unwrap();
    /// let bytes = keypair.into_bytes();
    /// let keypair2 = KeyPair::from_bytes(&bytes).unwrap();
    /// ```
    pub fn from_bytes(
        data: &[u8; Self::SECRET_KEY_BYTES_SIZE],
    ) -> Result<Self, MassaSignatureError> {
        let secret = ed25519_dalek::SecretKey::from_bytes(&data[..]).map_err(|err| {
            MassaSignatureError::ParsingError(format!("keypair bytes parsing error: {}", err))
        })?;
        Ok(KeyPair(ed25519_dalek::Keypair {
            public: ed25519_dalek::PublicKey::from(&secret),
            secret,
        }))
    }
}

#[transition::impl_version(versions("0", "1"), structures("KeyPair"))]
impl KeyPair {
    /// Return the bytes representing the keypair (should be a reference in the future)
    ///
    /// # Example
    /// ```
    /// # use massa_signature::KeyPair;
    /// let keypair = KeyPair::generate(0).unwrap();
    /// let bytes = keypair.to_bytes();
    /// ```
    pub fn to_bytes(&self) -> Vec<u8> {
        let version_serializer = U64VarIntSerializer::new();
        let mut bytes: Vec<u8> = Vec::new();
        version_serializer
            .serialize(&Self::VERSION, &mut bytes)
            .expect("Failed to serialize KeyPair version");
        bytes.extend_from_slice(&self.0.to_bytes());
        bytes
    }
}

impl ::serde::Serialize for KeyPair {
    /// `::serde::Serialize` trait for `KeyPair`
    /// if the serializer is human readable,
    /// serialization is done using `serialize_bs58_check`
    /// else, it uses `serialize_binary`
    ///
    /// # Example
    ///
    /// Human readable serialization :
    /// ```
    /// # use massa_signature::KeyPair;
    /// # use serde::{Deserialize, Serialize};
    /// let keypair = KeyPair::generate(0).unwrap();
    /// let serialized: String = serde_json::to_string(&keypair).unwrap();
    /// ```
    ///
    fn serialize<S: ::serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        let mut keypair_serializer = s.serialize_struct("keypair", 2)?;
        keypair_serializer.serialize_field("secret_key", &Cow::from(self.to_string()))?;
        keypair_serializer
            .serialize_field("public_key", &Cow::from(self.get_public_key().to_string()))?;
        keypair_serializer.end()
    }
}

impl<'de> ::serde::Deserialize<'de> for KeyPair {
    /// `::serde::Deserialize` trait for `KeyPair`
    /// if the deserializer is human readable,
    /// deserialization is done using `deserialize_bs58_check`
    /// else, it uses `deserialize_binary`
    ///
    /// # Example
    ///
    /// Human readable deserialization :
    /// ```
    /// # use massa_signature::KeyPair;
    /// # use serde::{Deserialize, Serialize};
    /// let keypair = KeyPair::generate(0).unwrap();
    /// let serialized = serde_json::to_string(&keypair).unwrap();
    /// let deserialized: KeyPair = serde_json::from_str(&serialized).unwrap();
    /// ```
    ///
    fn deserialize<D: ::serde::Deserializer<'de>>(d: D) -> Result<KeyPair, D::Error> {
        enum Field {
            SecretKey,
            PublicKey,
        }

        impl<'de> Deserialize<'de> for Field {
            fn deserialize<D>(deserializer: D) -> Result<Field, D::Error>
            where
                D: serde::Deserializer<'de>,
            {
                struct FieldVisitor;

                impl<'de> Visitor<'de> for FieldVisitor {
                    type Value = Field;

                    fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
                        formatter.write_str("`secret_key` or `public_key`")
                    }

                    fn visit_str<E>(self, value: &str) -> Result<Field, E>
                    where
                        E: serde::de::Error,
                    {
                        match value {
                            "secret_key" => Ok(Field::SecretKey),
                            "public_key" => Ok(Field::PublicKey),
                            _ => Err(serde::de::Error::unknown_field(value, FIELDS)),
                        }
                    }
                }

                deserializer.deserialize_identifier(FieldVisitor)
            }
        }

        struct KeyPairVisitor;

        impl<'de> Visitor<'de> for KeyPairVisitor {
            type Value = KeyPair;

            fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
                formatter.write_str("{'secret_key': 'xxx', 'public_key': 'xxx'}")
            }

            fn visit_seq<V>(self, mut seq: V) -> Result<KeyPair, V::Error>
            where
                V: SeqAccess<'de>,
            {
                let secret: Cow<str> = seq
                    .next_element()?
                    .ok_or_else(|| serde::de::Error::invalid_length(0, &self))?;
                let _: Cow<str> = seq
                    .next_element()?
                    .ok_or_else(|| serde::de::Error::invalid_length(1, &self))?;
                KeyPair::from_str(&secret).map_err(serde::de::Error::custom)
            }

            fn visit_map<V>(self, mut map: V) -> Result<KeyPair, V::Error>
            where
                V: MapAccess<'de>,
            {
                let mut secret = None;
                let mut public = None;
                while let Some(key) = map.next_key()? {
                    match key {
                        Field::SecretKey => {
                            if secret.is_some() {
                                return Err(serde::de::Error::duplicate_field("secret"));
                            }
                            secret = Some(map.next_value()?);
                        }
                        Field::PublicKey => {
                            if public.is_some() {
                                return Err(serde::de::Error::duplicate_field("public"));
                            }
                            public = Some(map.next_value()?);
                        }
                    }
                }
                let secret: Cow<str> =
                    secret.ok_or_else(|| serde::de::Error::missing_field("secret"))?;
                let _: Cow<str> =
                    public.ok_or_else(|| serde::de::Error::missing_field("public"))?;
                KeyPair::from_str(&secret).map_err(serde::de::Error::custom)
            }
        }

        const FIELDS: &[&str] = &["secret_key", "public_key"];
        d.deserialize_struct("KeyPair", FIELDS, KeyPairVisitor)
    }
}

#[allow(missing_docs)]
/// Public key used to check if a message was encoded
/// by the corresponding `PublicKey`.
/// Generated from the `KeyPair` using `SignatureEngine`
#[transition::versioned(versions("0", "1"))]
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct PublicKey(ed25519_dalek::PublicKey);

const PUBLIC_PREFIX: char = 'P';

#[transition::impl_version(versions("0", "1"), structures("PublicKey"))]
impl PublicKey {
    /// Size of a public key
    pub const PUBLIC_KEY_SIZE_BYTES: usize = ed25519_dalek::PUBLIC_KEY_LENGTH;
}

#[allow(clippy::derived_hash_with_manual_eq)]
impl std::hash::Hash for PublicKey {
    fn hash<H: Hasher>(&self, state: &mut H) {
        match self {
            PublicKey::PublicKeyV0(pubkey) => pubkey.hash(state),
            PublicKey::PublicKeyV1(pubkey) => pubkey.hash(state),
        }
    }
}

impl PartialOrd for PublicKey {
    fn partial_cmp(&self, other: &PublicKey) -> Option<Ordering> {
        match (self, other) {
            (PublicKey::PublicKeyV0(pubkey), PublicKey::PublicKeyV0(other_pubkey)) => {
                pubkey.0.as_bytes().partial_cmp(other_pubkey.0.as_bytes())
            }
            (PublicKey::PublicKeyV0(pubkey), PublicKey::PublicKeyV1(other_pubkey)) => {
                pubkey.0.as_bytes().partial_cmp(&other_pubkey.0.as_bytes())
            }
            (PublicKey::PublicKeyV1(pubkey), PublicKey::PublicKeyV0(other_pubkey)) => {
                pubkey.0.as_bytes().partial_cmp(other_pubkey.0.as_bytes())
            }
            (PublicKey::PublicKeyV1(pubkey), PublicKey::PublicKeyV1(other_pubkey)) => {
                pubkey.0.as_bytes().partial_cmp(&other_pubkey.0.as_bytes())
            }
        }
    }
}

impl Ord for PublicKey {
    fn cmp(&self, other: &PublicKey) -> Ordering {
        match (self, other) {
            (PublicKey::PublicKeyV0(pubkey), PublicKey::PublicKeyV0(other_pubkey)) => {
                pubkey.0.as_bytes().cmp(other_pubkey.0.as_bytes())
            }
            (PublicKey::PublicKeyV0(pubkey), PublicKey::PublicKeyV1(other_pubkey)) => {
                pubkey.0.as_bytes().cmp(&other_pubkey.0.as_bytes())
            }
            (PublicKey::PublicKeyV1(pubkey), PublicKey::PublicKeyV0(other_pubkey)) => {
                pubkey.0.as_bytes().cmp(other_pubkey.0.as_bytes())
            }
            (PublicKey::PublicKeyV1(pubkey), PublicKey::PublicKeyV1(other_pubkey)) => {
                pubkey.0.as_bytes().cmp(&other_pubkey.0.as_bytes())
            }
        }
    }
}

impl std::fmt::Display for PublicKey {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        match self {
            PublicKey::PublicKeyV0(pubkey) => pubkey.fmt(f),
            PublicKey::PublicKeyV1(pubkey) => pubkey.fmt(f),
        }
    }
}

impl std::fmt::Debug for PublicKey {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(f, "{}", self)
    }
}

impl FromStr for PublicKey {
    type Err = MassaSignatureError;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let mut chars = s.chars();
        match chars.next() {
            Some(prefix) if prefix == PUBLIC_PREFIX => {
                let data = chars.collect::<String>();
                let decoded_bs58_check =
                    bs58::decode(data)
                        .with_check(None)
                        .into_vec()
                        .map_err(|_| {
                            MassaSignatureError::ParsingError("Bad public key bs58".to_owned())
                        })?;
                let u64_deserializer = U64VarIntDeserializer::new(Included(0), Included(u64::MAX));
                let (rest, version) = u64_deserializer
                    .deserialize::<DeserializeError>(&decoded_bs58_check[..])
                    .map_err(|err| MassaSignatureError::ParsingError(err.to_string()))?;

                match version {
                    <PublicKey!["0"]>::VERSION => Ok(PublicKeyVariant!["0"](
                        <PublicKey!["0"]>::from_bytes(rest.try_into().map_err(|_| {
                            MassaSignatureError::ParsingError(format!(
                                "public key not long enough for: {}",
                                s
                            ))
                        })?)?,
                    )),
                    <PublicKey!["1"]>::VERSION => Ok(PublicKeyVariant!["1"](
                        <PublicKey!["1"]>::from_bytes(rest.try_into().map_err(|_| {
                            MassaSignatureError::ParsingError(format!(
                                "public key not long enough for: {}",
                                s
                            ))
                        })?)?,
                    )),
                    _ => Err(MassaSignatureError::InvalidVersionError(String::from(
                        "Unknown PublicKey version",
                    ))),
                }
            }
            _ => Err(MassaSignatureError::ParsingError(
                "Bad public key prefix".to_owned(),
            )),
        }
    }
}

impl PublicKey {
    /// Checks if the `Signature` associated with data bytes
    /// was produced with the `KeyPair` associated to given `PublicKey`
    pub fn verify_signature(
        &self,
        hash: &Hash,
        signature: &Signature,
    ) -> Result<(), MassaSignatureError> {
        match (self, signature) {
            (PublicKey::PublicKeyV0(pubkey), Signature::SignatureV0(signature)) => {
                pubkey.verify_signature(hash, signature)
            }
            (PublicKey::PublicKeyV1(pubkey), Signature::SignatureV1(signature)) => {
                pubkey.verify_signature(hash, signature)
            }
            _ => Err(MassaSignatureError::InvalidVersionError(String::from(
                "The PublicKey and Signature versions do not match",
            ))),
        }
    }

    /// Serialize a `PublicKey` as bytes.
    ///
    /// # Example
    ///  ```
    /// # use massa_signature::{PublicKey, KeyPair};
    /// # use serde::{Deserialize, Serialize};
    /// let keypair = KeyPair::generate(0).unwrap();
    ///
    /// let serialize = keypair.get_public_key().to_bytes();
    /// ```
    pub fn to_bytes(&self) -> Vec<u8> {
        match self {
            PublicKey::PublicKeyV0(pubkey) => pubkey.to_bytes(),
            PublicKey::PublicKeyV1(pubkey) => pubkey.to_bytes(),
        }
    }

    /// Deserialize a `PublicKey` from bytes.
    ///
    /// # Example
    ///  ```
    /// # use massa_signature::{PublicKey, KeyPair};
    /// # use serde::{Deserialize, Serialize};
    /// let keypair = KeyPair::generate(0).unwrap();
    ///
    /// let serialized = keypair.get_public_key().into_bytes();
    /// let deserialized: PublicKey = PublicKey::from_bytes(&serialized).unwrap();
    /// ```
    pub fn from_bytes(data: &[u8]) -> Result<PublicKey, MassaSignatureError> {
        let u64_deserializer = U64VarIntDeserializer::new(Included(0), Included(u64::MAX));
        let (rest, version) = u64_deserializer
            .deserialize::<DeserializeError>(data)
            .map_err(|err| MassaSignatureError::ParsingError(err.to_string()))?;
        match version {
            <PublicKey!["0"]>::VERSION => Ok(PublicKeyVariant!["0"](
                <PublicKey!["0"]>::from_bytes(rest.try_into().map_err(|err| {
                    MassaSignatureError::ParsingError(format!(
                        "pubkey bytes parsing error: {}",
                        err
                    ))
                })?)?,
            )),
            <PublicKey!["1"]>::VERSION => Ok(PublicKeyVariant!["1"](
                <PublicKey!["1"]>::from_bytes(rest.try_into().map_err(|err| {
                    MassaSignatureError::ParsingError(format!(
                        "pubkey bytes parsing error: {}",
                        err
                    ))
                })?)?,
            )),
            _ => Ok(PublicKeyVariant!["0"](<PublicKey!["0"]>::from_bytes(
                data.try_into().map_err(|err| {
                    MassaSignatureError::ParsingError(format!(
                        "pubkey bytes parsing error: {}",
                        err
                    ))
                })?,
            )?)),
            /*_ => Err(MassaSignatureError::InvalidVersionError(format!(
                "Unknown PublicKey version: {}", version
            ))),*/
        }
    }
}

#[transition::impl_version(versions("0", "1"), structures("PublicKey"))]
impl PublicKey {
    /// Return the bytes representing the keypair (should be a reference in the future)
    ///
    /// # Example
    /// ```
    /// # use massa_signature::KeyPair;
    /// let keypair = KeyPair::generate(0).unwrap();
    /// let bytes = keypair.to_bytes();
    /// ```
    pub fn to_bytes(&self) -> Vec<u8> {
        let version_serializer = U64VarIntSerializer::new();
        let mut bytes: Vec<u8> =
            Vec::with_capacity(Self::VERSION_VARINT_SIZE_BYTES + Self::PUBLIC_KEY_SIZE_BYTES);
        version_serializer
            .serialize(&Self::VERSION, &mut bytes)
            .unwrap();
        bytes.extend_from_slice(&self.0.to_bytes());
        bytes
    }
}

#[transition::impl_version(versions("0", "1"))]
#[allow(clippy::derived_hash_with_manual_eq)]
impl std::hash::Hash for PublicKey {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.0.to_bytes().hash(state);
    }
}

#[transition::impl_version(versions("0", "1"))]
impl PartialOrd for PublicKey {
    fn partial_cmp(&self, other: &PublicKey) -> Option<Ordering> {
        self.0.to_bytes().partial_cmp(&other.0.to_bytes())
    }
}

#[transition::impl_version(versions("0", "1"))]
impl Ord for PublicKey {
    fn cmp(&self, other: &PublicKey) -> Ordering {
        self.0.to_bytes().cmp(&other.0.to_bytes())
    }
}

#[transition::impl_version(versions("0", "1"))]
impl std::fmt::Display for PublicKey {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        let u64_serializer = U64VarIntSerializer::new();
        let mut bytes = Vec::new();
        u64_serializer
            .serialize(&Self::VERSION, &mut bytes)
            .map_err(|_| std::fmt::Error)?;
        bytes.extend(self.0.to_bytes());
        write!(
            f,
            "{}{}",
            PUBLIC_PREFIX,
            bs58::encode(bytes).with_check().into_string()
        )
    }
}

#[transition::impl_version(versions("0", "1"))]
impl std::fmt::Debug for PublicKey {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(f, "{}", self)
    }
}

#[transition::impl_version(versions("0", "1"), structures("PublicKey", "Signature"))]
impl PublicKey {
    /// Checks if the `Signature` associated with data bytes
    /// was produced with the `KeyPair` associated to given `PublicKey`
    pub fn verify_signature(
        &self,
        hash: &Hash,
        signature: &Signature,
    ) -> Result<(), MassaSignatureError> {
        self.0.verify(hash.to_bytes(), &signature.0).map_err(|err| {
            MassaSignatureError::SignatureError(format!("Signature verification failed: {}", err))
        })
    }

    /// Deserialize a `PublicKey` from bytes.
    ///
    /// # Example
    ///  ```
    /// # use massa_signature::{PublicKey, KeyPair};
    /// # use serde::{Deserialize, Serialize};
    /// let keypair = KeyPair::generate(0).unwrap();
    ///
    /// let serialized = keypair.get_public_key().into_bytes();
    /// let deserialized: PublicKey = PublicKey::from_bytes(&serialized).unwrap();
    /// ```
    pub fn from_bytes(
        data: &[u8; Self::PUBLIC_KEY_SIZE_BYTES],
    ) -> Result<PublicKey, MassaSignatureError> {
        ed25519_dalek::PublicKey::from_bytes(data)
            .map(|d| Self(d))
            .map_err(|err| MassaSignatureError::ParsingError(err.to_string()))
    }
}

/// Serializer for `Signature`
#[derive(Default)]
pub struct PublicKeyDeserializer;

impl PublicKeyDeserializer {
    /// Creates a `SignatureDeserializer`
    pub const fn new() -> Self {
        Self
    }
}

impl Deserializer<PublicKey> for PublicKeyDeserializer {
    /// ```
    /// use massa_signature::{PublicKey, PublicKeyDeserializer, KeyPair};
    /// use massa_serialization::{DeserializeError, Deserializer};
    /// use massa_hash::Hash;
    ///
    /// let keypair = KeyPair::generate(0).unwrap();
    /// let public_key = keypair.get_public_key();
    /// let serialized = public_key.to_bytes();
    /// let (rest, deser_public_key) = PublicKeyDeserializer::new().deserialize::<DeserializeError>(serialized).unwrap();
    /// assert!(rest.is_empty());
    /// assert_eq!(keypair.get_public_key(), deser_public_key);
    /// ```
    fn deserialize<'a, E: ParseError<&'a [u8]> + ContextError<&'a [u8]>>(
        &self,
        buffer: &'a [u8],
    ) -> IResult<&'a [u8], PublicKey, E> {
        // Can't use try into directly because it fails if there is more data in the buffer
        if buffer.len() < PUBLIC_KEY_SIZE_BYTES {
            return Err(nom::Err::Error(ParseError::from_error_kind(
                buffer,
                nom::error::ErrorKind::LengthValue,
            )));
        }
        let key =
            PublicKey::from_bytes(buffer[..PUBLIC_KEY_SIZE_BYTES].try_into().map_err(|_| {
                nom::Err::Error(ParseError::from_error_kind(
                    buffer,
                    nom::error::ErrorKind::LengthValue,
                ))
            })?)
            .map_err(|_| {
                nom::Err::Error(ParseError::from_error_kind(
                    buffer,
                    nom::error::ErrorKind::Fail,
                ))
            })?;
        // Safe because the signature deserialization success
        Ok((&buffer[PUBLIC_KEY_SIZE_BYTES..], key))
    }
}

impl ::serde::Serialize for PublicKey {
    /// `::serde::Serialize` trait for `PublicKey`
    /// if the serializer is human readable,
    /// serialization is done using `serialize_bs58_check`
    /// else, it uses `serialize_binary`
    ///
    /// # Example
    ///
    /// Human readable serialization :
    /// ```
    /// # use massa_signature::KeyPair;
    /// # use serde::{Deserialize, Serialize};
    /// let keypair = KeyPair::generate(0).unwrap();
    /// let serialized: String = serde_json::to_string(&keypair.get_public_key()).unwrap();
    /// ```
    ///
    fn serialize<S: ::serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.collect_str(&self.to_string())
    }
}

impl<'de> ::serde::Deserialize<'de> for PublicKey {
    /// `::serde::Deserialize` trait for `PublicKey`
    /// if the deserializer is human readable,
    /// deserialization is done using `deserialize_bs58_check`
    /// else, it uses `deserialize_binary`
    ///
    /// # Example
    ///
    /// Human readable deserialization :
    /// ```
    /// # use massa_signature::{PublicKey, KeyPair};
    /// # use serde::{Deserialize, Serialize};
    /// let keypair = KeyPair::generate(0).unwrap();
    ///
    /// let serialized = serde_json::to_string(&keypair.get_public_key()).unwrap();
    /// let deserialized: PublicKey = serde_json::from_str(&serialized).unwrap();
    /// ```
    ///
    fn deserialize<D: ::serde::Deserializer<'de>>(d: D) -> Result<PublicKey, D::Error> {
        struct Base58CheckVisitor;

        impl<'de> ::serde::de::Visitor<'de> for Base58CheckVisitor {
            type Value = PublicKey;

            fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
                formatter.write_str("an ASCII base58check string")
            }

            fn visit_bytes<E>(self, v: &[u8]) -> Result<Self::Value, E>
            where
                E: ::serde::de::Error,
            {
                if let Ok(v_str) = std::str::from_utf8(v) {
                    PublicKey::from_str(v_str).map_err(E::custom)
                } else {
                    Err(E::invalid_value(::serde::de::Unexpected::Bytes(v), &self))
                }
            }

            fn visit_str<E>(self, v: &str) -> Result<Self::Value, E>
            where
                E: ::serde::de::Error,
            {
                PublicKey::from_str(v).map_err(E::custom)
            }
        }
        d.deserialize_str(Base58CheckVisitor)
    }
}

#[allow(missing_docs)]
/// Signature generated from a message and a `KeyPair`.
#[transition::versioned(versions("0", "1"))]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Signature(ed25519_dalek::Signature);

#[transition::impl_version(versions("0", "1"), structures("Signature"))]
impl Signature {
    /// Size of a signature
    pub const SIGNATURE_SIZE_BYTES: usize = ed25519_dalek::SIGNATURE_LENGTH;
}

impl std::fmt::Display for Signature {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        match self {
            Signature::SignatureV0(signature) => signature.fmt(f),
            Signature::SignatureV1(signature) => signature.fmt(f),
        }
    }
}

impl FromStr for Signature {
    type Err = MassaSignatureError;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let data = s.chars().collect::<String>();
        let decoded_bs58_check = bs58::decode(data)
            .with_check(None)
            .into_vec()
            .map_err(|_| MassaSignatureError::ParsingError(format!("bad signature bs58: {}", s)))?;
        let u64_deserializer = U64VarIntDeserializer::new(Included(0), Included(u64::MAX));
        let (rest, version) = u64_deserializer
            .deserialize::<DeserializeError>(&decoded_bs58_check[..])
            .map_err(|err| MassaSignatureError::ParsingError(err.to_string()))?;
        match version {
            <Signature!["0"]>::VERSION => Ok(SignatureVariant!["0"](
                <Signature!["0"]>::from_bytes(rest.try_into().map_err(|_| {
                    MassaSignatureError::ParsingError(format!(
                        "signature not long enough for: {}",
                        s
                    ))
                })?)?,
            )),
            <Signature!["1"]>::VERSION => Ok(SignatureVariant!["1"](
                <Signature!["1"]>::from_bytes(rest.try_into().map_err(|_| {
                    MassaSignatureError::ParsingError(format!(
                        "signature not long enough for: {}",
                        s
                    ))
                })?)?,
            )),
            _ => Err(MassaSignatureError::InvalidVersionError(format!(
                "Unknown signature version: {}",
                version
            ))),
        }
    }
}

impl Signature {
    /// Serialize a `Signature` using `bs58` encoding with checksum.
    ///
    /// # Example
    ///  ```
    /// # use massa_signature::KeyPair;
    /// # use massa_hash::Hash;
    /// # use serde::{Deserialize, Serialize};
    /// let keypair = KeyPair::generate(0).unwrap();
    /// let data = Hash::compute_from("Hello World!".as_bytes());
    /// let signature = keypair.sign(&data).unwrap();
    ///
    /// let serialized: String = signature.to_bs58_check();
    /// ```
    pub fn to_bs58_check(&self) -> String {
        match self {
            Signature::SignatureV0(signature) => signature.to_bs58_check(),
            Signature::SignatureV1(signature) => signature.to_bs58_check(),
        }
    }

    /// Deserialize a `Signature` using `bs58` encoding with checksum.
    ///
    /// # Example
    ///  ```
    /// # use massa_signature::{KeyPair, Signature};
    /// # use massa_hash::Hash;
    /// # use serde::{Deserialize, Serialize};
    /// let keypair = KeyPair::generate(0).unwrap();
    /// let data = Hash::compute_from("Hello World!".as_bytes());
    /// let signature = keypair.sign(&data).unwrap();
    ///
    /// let serialized: String = signature.to_bs58_check();
    /// let deserialized: Signature = Signature::from_bs58_check(&serialized).unwrap();
    /// ```
    pub fn from_bs58_check(data: &str) -> Result<Signature, MassaSignatureError> {
        bs58::decode(data)
            .with_check(None)
            .into_vec()
            .map_err(|err| {
                MassaSignatureError::ParsingError(format!(
                    "signature bs58_check parsing error: {}",
                    err
                ))
            })
            .and_then(|signature| Signature::from_bytes(signature.as_slice()))
    }

    /// Serialize a Signature into bytes.
    ///
    /// # Example
    ///  ```
    /// # use massa_signature::KeyPair;
    /// # use massa_hash::Hash;
    /// # use serde::{Deserialize, Serialize};
    /// let keypair = KeyPair::generate(0).unwrap();
    /// let data = Hash::compute_from("Hello World!".as_bytes());
    /// let signature = keypair.sign(&data).unwrap();
    ///
    /// let serialized = signature.into_bytes();
    /// ```
    pub fn to_bytes(&self) -> Vec<u8> {
        match self {
            Signature::SignatureV0(signature) => signature.to_bytes(),
            Signature::SignatureV1(signature) => signature.to_bytes(),
        }
    }

    /// Deserialize a Signature from bytes.
    ///
    /// # Example
    ///  ```
    /// # use massa_signature::{KeyPair, Signature};
    /// # use massa_hash::Hash;
    /// # use serde::{Deserialize, Serialize};
    /// let keypair = KeyPair::generate(0).unwrap();
    /// let data = Hash::compute_from("Hello World!".as_bytes());
    /// let signature = keypair.sign(&data).unwrap();
    ///
    /// let serialized = signature.into_bytes();
    /// let deserialized: Signature = Signature::from_bytes(&serialized).unwrap();
    /// ```
    pub fn from_bytes(data: &[u8]) -> Result<Self, MassaSignatureError> {
        let u64_deserializer = U64VarIntDeserializer::new(Included(0), Included(u64::MAX));
        let (rest, version) = u64_deserializer
            .deserialize::<DeserializeError>(data)
            .map_err(|err| MassaSignatureError::ParsingError(err.to_string()))?;
        match version {
            <Signature!["0"]>::VERSION => Ok(SignatureVariant!["0"](
                <Signature!["0"]>::from_bytes(rest.try_into().map_err(|err| {
                    MassaSignatureError::ParsingError(format!(
                        "signature bytes parsing error: {}",
                        err
                    ))
                })?)?,
            )),
            <Signature!["1"]>::VERSION => Ok(SignatureVariant!["1"](
                <Signature!["1"]>::from_bytes(rest.try_into().map_err(|err| {
                    MassaSignatureError::ParsingError(format!(
                        "signature bytes parsing error: {}",
                        err
                    ))
                })?)?,
            )),
            _ => Err(MassaSignatureError::InvalidVersionError(format!(
                "Unknown signature version: {}",
                version
            ))),
        }
    }
}

#[transition::impl_version(versions("0", "1"))]
impl std::fmt::Display for Signature {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(f, "{}", self.to_bs58_check())
    }
}

#[transition::impl_version(versions("0", "1"), structures("Signature"))]
impl Signature {
    /// Serialize a `Signature` using `bs58` encoding with checksum.
    ///
    /// # Example
    ///  ```
    /// # use massa_signature::KeyPair;
    /// # use massa_hash::Hash;
    /// # use serde::{Deserialize, Serialize};
    /// let keypair = KeyPair::generate(0).unwrap();
    /// let data = Hash::compute_from("Hello World!".as_bytes());
    /// let signature = keypair.sign(&data).unwrap();
    ///
    /// let serialized: String = signature.to_bs58_check();
    /// ```
    pub fn to_bs58_check(self) -> String {
        bs58::encode(self.to_bytes()).with_check().into_string()
    }

    /// Serialize a Signature into bytes.
    ///
    /// # Example
    ///  ```
    /// # use massa_signature::KeyPair;
    /// # use massa_hash::Hash;
    /// # use serde::{Deserialize, Serialize};
    /// let keypair = KeyPair::generate(0).unwrap();
    /// let data = Hash::compute_from("Hello World!".as_bytes());
    /// let signature = keypair.sign(&data).unwrap();
    ///
    /// let serialized = signature.into_bytes();
    /// ```
    pub fn to_bytes(self) -> Vec<u8> {
        let version_serializer = U64VarIntSerializer::new();
        let mut bytes: Vec<u8> = Vec::new();
        version_serializer
            .serialize(&Self::VERSION, &mut bytes)
            .unwrap();
        bytes.extend_from_slice(&self.0.to_bytes());
        bytes
    }

    /// Deserialize a `Signature` using `bs58` encoding with checksum.
    ///
    /// # Example
    ///  ```
    /// # use massa_signature::{KeyPair, Signature};
    /// # use massa_hash::Hash;
    /// # use serde::{Deserialize, Serialize};
    /// let keypair = KeyPair::generate(0).unwrap();
    /// let data = Hash::compute_from("Hello World!".as_bytes());
    /// let signature = keypair.sign(&data).unwrap();
    ///
    /// let serialized: String = signature.to_bs58_check();
    /// let deserialized: Signature = Signature::from_bs58_check(&serialized).unwrap();
    /// ```
    pub fn from_bs58_check(data: &str) -> Result<Signature, MassaSignatureError> {
        bs58::decode(data)
            .with_check(None)
            .into_vec()
            .map_err(|err| {
                MassaSignatureError::ParsingError(format!(
                    "signature bs58_check parsing error: {}",
                    err
                ))
            })
            .and_then(|signature| {
                Signature::from_bytes(&signature.try_into().map_err(|err| {
                    MassaSignatureError::ParsingError(format!(
                        "signature bs58_check parsing error: {:?}",
                        err
                    ))
                })?)
            })
    }
}

#[transition::impl_version(versions("0", "1"), structures("Signature"))]
impl Signature {
    /// Deserialize a Signature from bytes.
    ///
    /// # Example
    ///  ```
    /// # use massa_signature::{KeyPair, Signature};
    /// # use massa_hash::Hash;
    /// # use serde::{Deserialize, Serialize};
    /// let keypair = KeyPair::generate(0).unwrap();
    /// let data = Hash::compute_from("Hello World!".as_bytes());
    /// let signature = keypair.sign(&data).unwrap();
    ///
    /// let serialized = signature.into_bytes();
    /// let deserialized: Signature = Signature::from_bytes(&serialized).unwrap();
    /// ```
    pub fn from_bytes(
        data: &[u8; Self::SIGNATURE_SIZE_BYTES],
    ) -> Result<Signature, MassaSignatureError> {
        ed25519_dalek::Signature::from_bytes(&data[..])
            .map(|s| Self(s))
            .map_err(|err| {
                MassaSignatureError::ParsingError(format!("signature bytes parsing error: {}", err))
            })
    }
}

impl ::serde::Serialize for Signature {
    /// `::serde::Serialize` trait for `Signature`
    /// if the serializer is human readable,
    /// serialization is done using `to_bs58_check`
    /// else, it uses `to_bytes`
    ///
    /// # Example
    ///
    /// Human readable serialization :
    /// ```
    /// # use massa_signature::{KeyPair, Signature};
    /// # use massa_hash::Hash;
    /// # use serde::{Deserialize, Serialize};
    /// let keypair = KeyPair::generate(0).unwrap();
    /// let data = Hash::compute_from("Hello World!".as_bytes());
    /// let signature = keypair.sign(&data).unwrap();
    ///
    /// let serialized: String = serde_json::to_string(&signature).unwrap();
    /// ```
    ///
    fn serialize<S: ::serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        if s.is_human_readable() {
            s.collect_str(&self.to_bs58_check())
        } else {
            s.serialize_bytes(self.to_bytes().as_ref())
        }
    }
}

impl<'de> ::serde::Deserialize<'de> for Signature {
    /// `::serde::Deserialize` trait for `Signature`
    /// if the deserializer is human readable,
    /// deserialization is done using `from_bs58_check`
    /// else, it uses `from_bytes`
    ///
    /// # Example
    ///
    /// Human readable deserialization :
    /// ```
    /// # use massa_signature::{KeyPair, Signature};
    /// # use massa_hash::Hash;
    /// # use serde::{Deserialize, Serialize};
    /// let keypair = KeyPair::generate(0).unwrap();
    /// let data = Hash::compute_from("Hello World!".as_bytes());
    /// let signature = keypair.sign(&data).unwrap();
    ///
    /// let serialized = serde_json::to_string(&signature).unwrap();
    /// let deserialized: Signature = serde_json::from_str(&serialized).unwrap();
    /// ```
    ///
    fn deserialize<D: ::serde::Deserializer<'de>>(d: D) -> Result<Signature, D::Error> {
        if d.is_human_readable() {
            struct SignatureVisitor;

            impl<'de> ::serde::de::Visitor<'de> for SignatureVisitor {
                type Value = Signature;

                fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
                    formatter.write_str("an ASCII base58check string")
                }

                fn visit_bytes<E>(self, v: &[u8]) -> Result<Self::Value, E>
                where
                    E: ::serde::de::Error,
                {
                    if let Ok(v_str) = std::str::from_utf8(v) {
                        Signature::from_str(v_str).map_err(E::custom)
                    } else {
                        Err(E::invalid_value(::serde::de::Unexpected::Bytes(v), &self))
                    }
                }

                fn visit_str<E>(self, v: &str) -> Result<Self::Value, E>
                where
                    E: ::serde::de::Error,
                {
                    Signature::from_str(v).map_err(E::custom)
                }
            }
            d.deserialize_str(SignatureVisitor)
        } else {
            struct BytesVisitor;

            impl<'de> ::serde::de::Visitor<'de> for BytesVisitor {
                type Value = Signature;

                fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
                    formatter.write_str("a bytestring")
                }

                fn visit_bytes<E>(self, v: &[u8]) -> Result<Self::Value, E>
                where
                    E: ::serde::de::Error,
                {
                    Signature::from_bytes(v).map_err(E::custom)
                }
            }

            d.deserialize_bytes(BytesVisitor)
        }
    }
}

/// Serializer for `Signature`
#[derive(Default)]
pub struct SignatureDeserializer;

impl SignatureDeserializer {
    /// Creates a `SignatureDeserializer`
    pub const fn new() -> Self {
        Self
    }
}

impl Deserializer<Signature> for SignatureDeserializer {
    /// ```
    /// use massa_signature::{Signature, SignatureDeserializer, KeyPair};
    /// use massa_serialization::{DeserializeError, Deserializer};
    /// use massa_hash::Hash;
    ///
    /// let keypair = KeyPair::generate(0).unwrap();
    /// let data = Hash::compute_from("Hello World!".as_bytes());
    /// let signature = keypair.sign(&data).unwrap();
    /// let serialized = signature.into_bytes();
    /// let (rest, deser_signature) = SignatureDeserializer::new().deserialize::<DeserializeError>(&serialized).unwrap();
    /// assert!(rest.is_empty());
    /// assert_eq!(signature, deser_signature);
    /// ```
    fn deserialize<'a, E: ParseError<&'a [u8]> + ContextError<&'a [u8]>>(
        &self,
        buffer: &'a [u8],
    ) -> IResult<&'a [u8], Signature, E> {
        // Can't use try into directly because it fails if there is more data in the buffer
        if buffer.len() < SIGNATURE_SIZE_BYTES {
            return Err(nom::Err::Error(ParseError::from_error_kind(
                buffer,
                nom::error::ErrorKind::LengthValue,
            )));
        }
        let signature = Signature::from_bytes(buffer[..SIGNATURE_SIZE_BYTES].try_into().unwrap())
            .map_err(|_| {
            nom::Err::Error(ParseError::from_error_kind(
                buffer,
                nom::error::ErrorKind::Fail,
            ))
        })?;
        // Safe because the signature deserialization success
        Ok((&buffer[SIGNATURE_SIZE_BYTES..], signature))
    }
}

/// Verifies a batch of signatures
pub fn verify_signature_batch(
    batch: &[(Hash, Signature, PublicKey)],
) -> Result<(), MassaSignatureError> {
    //nothing to verify
    if batch.is_empty() {
        return Ok(());
    }

    // normal verif is fastest for size 1 batches
    if batch.len() == 1 {
        let (hash, signature, public_key) = batch[0];
        return public_key.verify_signature(&hash, &signature);
    }

    // otherwise, use batch verif.
    match batch[0] {
        (_hash, Signature::SignatureV0(_sig), PublicKey::PublicKeyV0(_pubkey)) => {
            let mut hashes = Vec::with_capacity(batch.len());
            let mut signatures = Vec::with_capacity(batch.len());
            let mut public_keys = Vec::with_capacity(batch.len());
            batch.iter().for_each(|(hash, signature, public_key)| {
                if let (Signature::SignatureV0(sig), PublicKey::PublicKeyV0(pubkey)) =
                    (&signature, &public_key)
                {
                    hashes.push(hash.to_bytes().as_slice());
                    signatures.push(sig.0);
                    public_keys.push(pubkey.0);
                }
            });
            ed25519_dalek::verify_batch(&hashes, signatures.as_slice(), public_keys.as_slice())
                .map_err(|err| {
                    MassaSignatureError::SignatureError(format!(
                        "Batch signature verification failed: {}",
                        err
                    ))
                })
        }
        (_hash, Signature::SignatureV1(_sig), PublicKey::PublicKeyV1(_pubkey)) => {
            let mut hashes = Vec::with_capacity(batch.len());
            let mut signatures = Vec::with_capacity(batch.len());
            let mut public_keys = Vec::with_capacity(batch.len());
            batch.iter().for_each(|(hash, signature, public_key)| {
                if let (Signature::SignatureV0(sig), PublicKey::PublicKeyV0(pubkey)) =
                    (&signature, &public_key)
                {
                    hashes.push(hash.to_bytes().as_slice());
                    signatures.push(sig.0);
                    public_keys.push(pubkey.0);
                }
            });
            ed25519_dalek::verify_batch(&hashes, signatures.as_slice(), public_keys.as_slice())
                .map_err(|err| {
                    MassaSignatureError::SignatureError(format!(
                        "Batch signature verification failed: {}",
                        err
                    ))
                })
        }
        _ => Err(MassaSignatureError::InvalidVersionError(String::from(
            "Batch contains incompatible versions",
        ))),
    }
}

#[transition::impl_version(versions("0", "1"), structures("Signature", "PublicKey"))]
impl Signature {
    /// Verify a batch of signatures on a single core to gain total CPU performance.
    /// Every provided triplet `(hash, signature, public_key)` is verified
    /// and an error is returned if at least one of them fails.
    ///
    /// # Arguments
    /// * `batch`: a slice of triplets `(hash, signature, public_key)`
    ///
    /// # Return value
    /// Returns `Ok(())` if all signatures were successfully verified,
    /// and `Err(MassaSignatureError::SignatureError(_))` if at least one of them failed.
    ///
    pub fn verify_signature_batch(
        batch: &[(Hash, Signature, PublicKey)],
    ) -> Result<(), MassaSignatureError> {
        // nothing to verify
        if batch.is_empty() {
            return Ok(());
        }

        // normal verif is fastest for size 1 batches
        if batch.len() == 1 {
            let (hash, signature, public_key) = batch[0];
            return public_key.verify_signature(&hash, &signature);
        }

        // otherwise, use batch verif
        let mut hashes = Vec::with_capacity(batch.len());
        let mut signatures = Vec::with_capacity(batch.len());
        let mut public_keys = Vec::with_capacity(batch.len());
        batch.iter().for_each(|(hash, signature, public_key)| {
            hashes.push(hash.to_bytes().as_slice());
            signatures.push(signature.0);
            public_keys.push(public_key.0);
        });
        ed25519_dalek::verify_batch(&hashes, signatures.as_slice(), public_keys.as_slice()).map_err(
            |err| {
                MassaSignatureError::SignatureError(format!(
                    "Batch signature verification failed: {}",
                    err
                ))
            },
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use massa_hash::Hash;
    use serial_test::serial;

    #[test]
    #[serial]
    fn test_example() {
        let keypair = KeyPair::generate(0).unwrap();
        let message = "Hello World!".as_bytes();
        let hash = Hash::compute_from(message);
        let signature = keypair.sign(&hash).unwrap();
        assert!(keypair
            .get_public_key()
            .verify_signature(&hash, &signature)
            .is_ok())
    }

    #[test]
    #[serial]
    fn test_serde_keypair() {
        let keypair = KeyPair::generate(0).unwrap();
        let serialized = serde_json::to_string(&keypair).expect("could not serialize keypair");
        let deserialized: KeyPair =
            serde_json::from_str(&serialized).expect("could not deserialize keypair");

        match (keypair, deserialized) {
            (KeyPair::KeyPairV0(keypair), KeyPair::KeyPairV0(deserialized)) => {
                assert_eq!(keypair.0.public, deserialized.0.public);
            }
            _ => {
                panic!("Wrong version provided");
            }
        }
    }

    #[test]
    #[serial]
    fn test_serde_public_key() {
        let keypair = KeyPair::generate(0).unwrap();
        let public_key = keypair.get_public_key();
        let serialized =
            serde_json::to_string(&public_key).expect("Could not serialize public key");
        let deserialized =
            serde_json::from_str(&serialized).expect("could not deserialize public key");
        assert_eq!(public_key, deserialized);
    }

    #[test]
    #[serial]
    fn test_serde_signature() {
        let keypair = KeyPair::generate(0).unwrap();
        let message = "Hello World!".as_bytes();
        let hash = Hash::compute_from(message);
        let signature = keypair.sign(&hash).unwrap();
        let serialized =
            serde_json::to_string(&signature).expect("could not serialize signature key");
        let deserialized =
            serde_json::from_str(&serialized).expect("could not deserialize signature key");
        assert_eq!(signature, deserialized);
    }
}
