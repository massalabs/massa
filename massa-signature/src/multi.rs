// Copyright (c) 2022 MASSA LABS <info@massa.net>

use crate::error::MassaSignatureError;
use anyhow::{bail, Error};

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
use tokio::join;

use transition::Versioned;

// TODO : REMOVE THESE DECLARATIONS AND HANDLE ERRORS!
pub const PUBLIC_KEY_SIZE_BYTES: usize = 32;
pub const SIGNATURE_SIZE_BYTES: usize = 32;

/// A versioned KeyPair
#[transition::versioned(versions("0", "1"))]
pub struct KeyPair {
    #[transition::field(versions("0"))]
    pub a: ed25519_dalek::Keypair,

    #[transition::field(versions("1"))]
    pub a: schnorrkel::Keypair,
}

#[transition::impl_version(versions("0"), structures("KeyPair"))]
impl KeyPair {
    /// Size of a keypair
    pub const SECRET_KEY_BYTES_SIZE: usize = ed25519_dalek::SECRET_KEY_LENGTH;
}

#[transition::impl_version(versions("1"), structures("KeyPair"))]
impl KeyPair {
    /// Size of a keypair
    pub const SECRET_KEY_BYTES_SIZE: usize = schnorrkel::SECRET_KEY_LENGTH;
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

#[transition::impl_version(versions("0"))]
impl Clone for KeyPair {
    fn clone(&self) -> Self {
        KeyPair {
            a: ed25519_dalek::Keypair {
                // This will never error since self is a valid keypair
                secret: ed25519_dalek::SecretKey::from_bytes(self.a.secret.as_bytes()).unwrap(),
                public: self.a.public,
            },
        }
    }
}

#[transition::impl_version(versions("1"))]
impl Clone for KeyPair {
    fn clone(&self) -> Self {
        KeyPair {
            a: schnorrkel::Keypair {
                // This will never error since self is a valid keypair
                secret: schnorrkel::SecretKey::from_bytes(&self.a.secret.to_bytes()).unwrap(),
                public: self.a.public,
            },
        }
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

#[transition::impl_version(versions("0"), structures("KeyPair", "Signature", "PublicKey"))]
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
        Ok(Signature {
            a: self.a.sign(hash.to_bytes()),
        })
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
        PublicKey { a: self.a.public }
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
        KeyPair {
            a: ed25519_dalek::Keypair::generate(&mut rng),
        }
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
        Ok(KeyPair {
            a: ed25519_dalek::Keypair {
                public: ed25519_dalek::PublicKey::from(&secret),
                secret,
            },
        })
    }
}

#[transition::impl_version(versions("1"), structures("KeyPair", "Signature", "PublicKey"))]
impl KeyPair {
    /// Returns the Signature produced by signing
    /// data bytes with a `KeyPair`.
    ///
    /// # Example
    ///  ```
    /// # use massa_signature::KeyPair;
    /// # use massa_hash::Hash;
    /// let keypair = KeyPair::generate(2).unwrap();
    /// let data = Hash::compute_from("Hello World!".as_bytes());
    /// let signature = keypair.sign(&data).unwrap();
    /// ```
    pub fn sign(&self, hash: &Hash) -> Result<Signature, MassaSignatureError> {
        let t = schnorrkel::signing_context(b"massa_sign").bytes(hash.to_bytes());
        Ok(Signature { a: self.a.sign(t) })
    }

    /// Get the public key of the keypair
    ///
    /// # Example
    /// ```
    /// # use massa_signature::KeyPair;
    /// let keypair = KeyPair::generate(2).unwrap();
    /// let public_key = keypair.get_public_key();
    /// ```
    pub fn get_public_key(&self) -> PublicKey {
        PublicKey { a: self.a.public }
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
        KeyPair {
            a: schnorrkel::Keypair::generate(),
        }
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
        let secret = schnorrkel::SecretKey::from_bytes(&data[..]).map_err(|err| {
            MassaSignatureError::ParsingError(format!("keypair bytes parsing error: {}", err))
        })?;
        Ok(KeyPair {
            a: schnorrkel::Keypair {
                public: schnorrkel::PublicKey::from(secret.clone()),
                secret,
            },
        })
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
        bytes[Self::VERSION_VARINT_SIZE_BYTES..].copy_from_slice(&self.a.to_bytes());
        bytes
    }

    /// Return the bytes representing the keypair
    ///
    /// # Example
    /// ```
    /// # use massa_signature::KeyPair;
    /// let keypair = KeyPair::generate(0).unwrap();
    /// let bytes = keypair.into_bytes();
    /// ```
    pub fn into_bytes(self) -> [u8; Self::SECRET_KEY_BYTES_SIZE] {
        self.a.secret.to_bytes()
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

/// Public key used to check if a message was encoded
/// by the corresponding `PublicKey`.
/// Generated from the `KeyPair` using `SignatureEngine`
#[transition::versioned(versions("0", "1"))]
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct PublicKey {
    /// The ed25519_dalek public key to wrap
    #[transition::field(versions("0"))]
    pub a: ed25519_dalek::PublicKey,

    /// The schnorrkel public key to wrap
    #[transition::field(versions("1"))]
    pub a: schnorrkel::PublicKey,
}

const PUBLIC_PREFIX: char = 'P';

#[transition::impl_version(versions("0"), structures("PublicKey"))]
impl PublicKey {
    /// Size of a public key
    pub const PUBLIC_KEY_SIZE_BYTES: usize = ed25519_dalek::PUBLIC_KEY_LENGTH;
}

#[transition::impl_version(versions("1"), structures("PublicKey"))]
impl PublicKey {
    /// Size of a public key
    pub const PUBLIC_KEY_SIZE_BYTES: usize = schnorrkel::PUBLIC_KEY_LENGTH;
}

#[allow(clippy::derive_hash_xor_eq)]
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
                pubkey.a.as_bytes().partial_cmp(other_pubkey.a.as_bytes())
            }
            (PublicKey::PublicKeyV0(pubkey), PublicKey::PublicKeyV1(other_pubkey)) => {
                pubkey.a.as_bytes().partial_cmp(&other_pubkey.a.to_bytes())
            }
            (PublicKey::PublicKeyV1(pubkey), PublicKey::PublicKeyV0(other_pubkey)) => {
                pubkey.a.to_bytes().partial_cmp(other_pubkey.a.as_bytes())
            }
            (PublicKey::PublicKeyV1(pubkey), PublicKey::PublicKeyV1(other_pubkey)) => {
                pubkey.a.to_bytes().partial_cmp(&other_pubkey.a.to_bytes())
            }
        }
    }
}

impl Ord for PublicKey {
    fn cmp(&self, other: &PublicKey) -> Ordering {
        match (self, other) {
            (PublicKey::PublicKeyV0(pubkey), PublicKey::PublicKeyV0(other_pubkey)) => {
                pubkey.a.as_bytes().cmp(other_pubkey.a.as_bytes())
            }
            (PublicKey::PublicKeyV0(pubkey), PublicKey::PublicKeyV1(other_pubkey)) => {
                pubkey.a.as_bytes().cmp(&other_pubkey.a.to_bytes())
            }
            (PublicKey::PublicKeyV1(pubkey), PublicKey::PublicKeyV0(other_pubkey)) => {
                pubkey.a.to_bytes().cmp(other_pubkey.a.as_bytes())
            }
            (PublicKey::PublicKeyV1(pubkey), PublicKey::PublicKeyV1(other_pubkey)) => {
                pubkey.a.to_bytes().cmp(&other_pubkey.a.to_bytes())
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
        bytes.extend_from_slice(&self.a.to_bytes());
        bytes
    }
}

#[transition::impl_version(versions("0", "1"))]
#[allow(clippy::derive_hash_xor_eq)]
impl std::hash::Hash for PublicKey {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.a.to_bytes().hash(state);
    }
}

#[transition::impl_version(versions("0", "1"))]
impl PartialOrd for PublicKey {
    fn partial_cmp(&self, other: &PublicKey) -> Option<Ordering> {
        self.a.to_bytes().partial_cmp(&other.a.to_bytes())
    }
}

#[transition::impl_version(versions("0", "1"))]
impl Ord for PublicKey {
    fn cmp(&self, other: &PublicKey) -> Ordering {
        self.a.to_bytes().cmp(&other.a.to_bytes())
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
        bytes.extend(self.a.to_bytes());
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

#[transition::impl_version(versions("0"), structures("PublicKey", "Signature"))]
impl PublicKey {
    /// Checks if the `Signature` associated with data bytes
    /// was produced with the `KeyPair` associated to given `PublicKey`
    pub fn verify_signature(
        &self,
        hash: &Hash,
        signature: &Signature,
    ) -> Result<(), MassaSignatureError> {
        self.a.verify(hash.to_bytes(), &signature.a).map_err(|err| {
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
            .map(|d| Self { a: d })
            .map_err(|err| MassaSignatureError::ParsingError(err.to_string()))
    }
}

#[transition::impl_version(versions("1"), structures("PublicKey", "Signature"))]
impl PublicKey {
    /// Checks if the `Signature` associated with data bytes
    /// was produced with the `KeyPair` associated to given `PublicKey`
    pub fn verify_signature(
        &self,
        hash: &Hash,
        signature: &Signature,
    ) -> Result<(), MassaSignatureError> {
        let t = schnorrkel::signing_context(b"massa_sign").bytes(hash.to_bytes());

        self.a.verify(t, &signature.a).map_err(|err| {
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
        schnorrkel::PublicKey::from_bytes(data)
            .map(|d| Self { a: d })
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

/// Signature generated from a message and a `KeyPair`.
#[transition::versioned(versions("0", "1"))]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Signature {
    #[transition::field(versions("0"))]
    pub a: ed25519_dalek::Signature,

    #[transition::field(versions("1"))]
    pub a: schnorrkel::Signature,
}

#[transition::impl_version(versions("0"), structures("Signature"))]
impl Signature {
    /// Size of a signature
    pub const SIGNATURE_SIZE_BYTES: usize = ed25519_dalek::SIGNATURE_LENGTH;
}

#[transition::impl_version(versions("1"), structures("Signature"))]
impl Signature {
    /// Size of a signature
    pub const SIGNATURE_SIZE_BYTES: usize = schnorrkel::SIGNATURE_LENGTH;
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
    pub fn into_bytes(&self) -> Vec<u8> {
        match self {
            Signature::SignatureV0(signature) => signature.into_bytes(),
            Signature::SignatureV1(signature) => signature.into_bytes(),
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
    /// let serialized = signature.to_bytes();
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
        bs58::encode(self.into_bytes()).with_check().into_string()
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
    pub fn into_bytes(self) -> Vec<u8> {
        let version_serializer = U64VarIntSerializer::new();
        let mut bytes: Vec<u8> = Vec::new();
        version_serializer
            .serialize(&Self::VERSION, &mut bytes)
            .unwrap();
        bytes.extend_from_slice(&self.a.to_bytes());
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

#[transition::impl_version(versions("0"), structures("Signature"))]
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
    /// let serialized = signature.to_bytes();
    /// let deserialized: Signature = Signature::from_bytes(&serialized).unwrap();
    /// ```
    pub fn from_bytes(
        data: &[u8; Self::SIGNATURE_SIZE_BYTES],
    ) -> Result<Signature, MassaSignatureError> {
        ed25519_dalek::Signature::from_bytes(&data[..])
            .map(|s| Self { a: s })
            .map_err(|err| {
                MassaSignatureError::ParsingError(format!("signature bytes parsing error: {}", err))
            })
    }
}

#[transition::impl_version(versions("1"), structures("Signature"))]
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
    /// let serialized = signature.to_bytes();
    /// let deserialized: Signature = Signature::from_bytes(&serialized).unwrap();
    /// ```
    pub fn from_bytes(
        data: &[u8; Self::SIGNATURE_SIZE_BYTES],
    ) -> Result<Signature, MassaSignatureError> {
        schnorrkel::Signature::from_bytes(&data[..])
            .map(|s| Self { a: s })
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
            s.serialize_bytes(self.into_bytes().as_ref())
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
                    signatures.push(sig.a);
                    public_keys.push(pubkey.a);
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
            let mut ts = Vec::with_capacity(batch.len());
            let mut signatures = Vec::with_capacity(batch.len());
            let mut public_keys = Vec::with_capacity(batch.len());
            batch.iter().for_each(|(hash, signature, public_key)| {
                if let (Signature::SignatureV1(sig), PublicKey::PublicKeyV1(pubkey)) =
                    (&signature, &public_key)
                {
                    ts.push(schnorrkel::signing_context(b"massa_sign").bytes(hash.to_bytes()));
                    signatures.push(sig.a);
                    public_keys.push(pubkey.a);
                }
            });
            schnorrkel::verify_batch(ts, signatures.as_slice(), public_keys.as_slice(), false)
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

#[transition::impl_version(versions("0"), structures("Signature", "PublicKey"))]
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
            signatures.push(signature.a);
            public_keys.push(public_key.a);
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

#[transition::impl_version(versions("1"), structures("Signature", "PublicKey"))]
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

        let mut ts = Vec::with_capacity(batch.len());
        let mut signatures = Vec::with_capacity(batch.len());
        let mut public_keys = Vec::with_capacity(batch.len());
        batch.iter().for_each(|(hash, signature, public_key)| {
            let t = schnorrkel::signing_context(b"massa_sign").bytes(hash.to_bytes());
            ts.push(t);
            signatures.push(signature.a);
            public_keys.push(public_key.a);
        });
        schnorrkel::verify_batch(ts, signatures.as_slice(), public_keys.as_slice(), false).map_err(
            |err| {
                MassaSignatureError::SignatureError(format!(
                    "Batch signature verification failed: {}",
                    err
                ))
            },
        )
    }
}

/* BEGIN MULTI IMPL */

impl KeyPair {
    /// Creates a new multi-signature scheme with this KeyPair
    pub fn musig(
        &self,
        hash: Hash,
    ) -> Result<
        schnorrkel::musig::MuSig<
            impl schnorrkel::context::SigningTranscript + Clone,
            schnorrkel::musig::CommitStage<&schnorrkel::Keypair>,
        >,
        MassaSignatureError,
    > {
        match self {
            KeyPair::KeyPairV0(_kp) => Err(MassaSignatureError::InvalidVersionError(String::from(
                "MultiSig not available for KeyPairs V1",
            ))),
            KeyPair::KeyPairV1(kp) => {
                let t = schnorrkel::signing_context(b"massa_sign").bytes(hash.to_bytes());

                Ok(kp.a.musig(t))
            }
        }
    }
}

/// A MultiSig struct, wrapping the schnorrkel musig
struct MultiSig {
    musig_commit: Option<
        schnorrkel::musig::MuSig<
            merlin::Transcript,
            schnorrkel::musig::CommitStage<schnorrkel::Keypair>,
        >,
    >,
    musig_reveal: Option<
        schnorrkel::musig::MuSig<
            merlin::Transcript,
            schnorrkel::musig::RevealStage<schnorrkel::Keypair>,
        >,
    >,
    musig_cosig:
        Option<schnorrkel::musig::MuSig<merlin::Transcript, schnorrkel::musig::CosignStage>>,
    our_keypair: KeyPair,
    other_pubkeys: Vec<PublicKey>,
    hash: Hash,
}

impl MultiSig {
    pub fn new(
        our_keypair: KeyPair,
        other_pubkeys: Vec<PublicKey>,
        hash: Hash,
    ) -> Result<MultiSig, MassaSignatureError> {
        match our_keypair.clone() {
            KeyPair::KeyPairV0(_kp) => Err(MassaSignatureError::InvalidVersionError(String::from(
                "MultiSig not available for KeyPairs V0",
            ))),
            KeyPair::KeyPairV1(kp) => {
                let t = schnorrkel::signing_context(b"massa_sig").bytes(hash.to_bytes());
                let m = schnorrkel::musig::MuSig::new(kp.a, t);

                Ok(MultiSig {
                    musig_commit: Some(m),
                    musig_reveal: None,
                    musig_cosig: None,
                    our_keypair,
                    other_pubkeys,
                    hash,
                })
            }
        }
    }

    pub fn get_our_commitment(&self) -> schnorrkel::musig::Commitment {
        let c = self.musig_commit.as_ref().unwrap().our_commitment();
        c
    }

    pub fn set_other_commitment(
        &mut self,
        pubkey: PublicKey,
        c: schnorrkel::musig::Commitment,
    ) -> Result<(), MassaSignatureError> {
        match pubkey {
            PublicKey::PublicKeyV0(_) => Err(MassaSignatureError::InvalidVersionError(
                String::from("Multi-sig not implemented for this PublicKey version"),
            )),
            PublicKey::PublicKeyV1(pk) => {
                let r = self
                    .musig_commit
                    .as_mut()
                    .unwrap()
                    .add_their_commitment(pk.a, c);
                r.map_err(|_| {
                    MassaSignatureError::SignatureError(String::from(
                        "Multi-sig set other commitment failed",
                    ))
                })
            }
        }
    }

    pub fn get_our_reveal(&mut self) -> &schnorrkel::musig::Reveal {
        self.musig_reveal = Some(self.musig_commit.take().unwrap().reveal_stage());
        let r = self.musig_reveal.as_ref().unwrap().our_reveal();

        r
    }

    pub fn set_other_reveal(
        &mut self,
        pubkey: PublicKey,
        r: schnorrkel::musig::Reveal,
    ) -> Result<(), MassaSignatureError> {
        match pubkey {
            PublicKey::PublicKeyV0(_) => Err(MassaSignatureError::InvalidVersionError(
                String::from("Multi-sig not implemented for this PublicKey version"),
            )),
            PublicKey::PublicKeyV1(pk) => {
                let r = self
                    .musig_reveal
                    .as_mut()
                    .unwrap()
                    .add_their_reveal(pk.a, r);
                r.map_err(|_| {
                    MassaSignatureError::SignatureError(String::from(
                        "Multi-sig set other reveal failed",
                    ))
                })
            }
        }
    }

    pub fn get_our_cosignature(&mut self) -> schnorrkel::musig::Cosignature {
        self.musig_cosig = Some(self.musig_reveal.take().unwrap().cosign_stage());
        let s = self.musig_cosig.as_ref().unwrap().our_cosignature();

        s
    }

    pub fn set_other_cosignature(
        &mut self,
        pubkey: PublicKey,
        s: schnorrkel::musig::Cosignature,
    ) -> Result<(), MassaSignatureError> {
        match pubkey {
            PublicKey::PublicKeyV0(_) => Err(MassaSignatureError::InvalidVersionError(
                String::from("Multi-sig not implemented for this PublicKey version"),
            )),
            PublicKey::PublicKeyV1(pk) => {
                let r = self
                    .musig_cosig
                    .as_mut()
                    .unwrap()
                    .add_their_cosignature(pk.a, s);
                r.map_err(|_| {
                    MassaSignatureError::SignatureError(String::from(
                        "Multi-sig set other cosignature failed",
                    ))
                })
            }
        }
    }
}

/// A MultiSig Message, used to send and receive commitments, reveals and cosignatures
#[derive(Clone)]
pub enum MultiSigMsg {
    /// A MultiSig Commitment
    Commitment(PublicKey, schnorrkel::musig::Commitment),
    /// A MultiSig Reveal
    Reveal(PublicKey, schnorrkel::musig::Reveal),
    /// A MultiSig Cosignature
    Cosignature(PublicKey, schnorrkel::musig::Cosignature),
}

impl std::fmt::Debug for MultiSigMsg {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> Result<(), std::fmt::Error> {
        match self {
            MultiSigMsg::Commitment(pk, _c) => {
                f.write_fmt(format_args!("MultiSigMsg::Commitment from pk: {}", pk))
            }
            MultiSigMsg::Reveal(pk, _r) => {
                f.write_fmt(format_args!("MultiSigMsg::Reveal from pk: {}", pk))
            }
            MultiSigMsg::Cosignature(pk, _s) => {
                f.write_fmt(format_args!("MultiSigMsg::Cosignature from pk: {}", pk))
            }
        }
    }
}

pub async fn start_multi_signature_scheme() -> Result<(), Error> {
    let (tx, _) = tokio::sync::broadcast::channel::<MultiSigMsg>(3);

    let keypair_1 = KeyPair::generate(1).unwrap();
    let keypair_2 = KeyPair::generate(1).unwrap();
    let keypair_3 = KeyPair::generate(1).unwrap();

    let pubkey_1 = keypair_1.get_public_key();
    let pubkey_2 = keypair_2.get_public_key();
    let pubkey_3 = keypair_2.get_public_key();

    let hash = Hash::compute_from(b"SomeData");

    let handle1 = tokio::spawn({
        let keypair_1 = keypair_1.clone();
        let tx_clone = tx.clone();
        let rx_clone = tx_clone.subscribe();
        async move {
            //handle_multi_signature_one_node(keypair_1, vec![pubkey_2, pubkey_3], hash, tx1, vec![rx2_clone, rx3_clone]).await?;
            handle_multi_signature_one_node(
                keypair_1,
                vec![pubkey_2, pubkey_3],
                hash,
                tx_clone,
                rx_clone,
            )
            .await?;

            Result::<(), Error>::Ok(())
        }
    });

    let handle2 = tokio::spawn({
        let keypair_2 = keypair_2.clone();
        let tx_clone = tx.clone();
        let rx_clone = tx_clone.subscribe();
        async move {
            //handle_multi_signature_one_node(keypair_2, vec![pubkey_1, pubkey_3], hash, tx2, vec![rx1_clone, rx3_clone]).await?;
            handle_multi_signature_one_node(
                keypair_2,
                vec![pubkey_1, pubkey_3],
                hash,
                tx_clone,
                rx_clone,
            )
            .await?;

            Result::<(), Error>::Ok(())
        }
    });

    let handle3 = tokio::spawn({
        let keypair_3 = keypair_3.clone();
        let tx_clone = tx.clone();
        let rx_clone = tx_clone.subscribe();
        async move {
            handle_multi_signature_one_node(
                keypair_3,
                vec![pubkey_1, pubkey_2],
                hash,
                tx_clone,
                rx_clone,
            )
            .await?;

            Result::<(), Error>::Ok(())
        }
    });

    println!("ALL 3 tasks launched");

    let (res1, res2, res3) = join!(handle1, handle2, handle3);

    assert!(res1.is_ok() && res1.unwrap().is_ok());
    assert!(res2.is_ok() && res2.unwrap().is_ok());
    assert!(res3.is_ok() && res3.unwrap().is_ok());

    println!("ALL 3 tasks joined");

    Ok(())
}

pub async fn handle_multi_signature_one_node(
    our_keypair: KeyPair,
    other_pubkeys: Vec<PublicKey>,
    hash: Hash,
    tx: tokio::sync::broadcast::Sender<MultiSigMsg>,
    mut rx: tokio::sync::broadcast::Receiver<MultiSigMsg>,
) -> Result<(), Error> {
    let mut multi_sig = MultiSig::new(our_keypair.clone(), other_pubkeys.clone(), hash).unwrap();

    /* Commit stage */

    println!("KP: {} - Start commit stage", our_keypair.get_public_key());

    // Send our commitment to other keypairs
    tx.send(MultiSigMsg::Commitment(
        our_keypair.get_public_key(),
        multi_sig.get_our_commitment(),
    ))?;

    // Wait for every other commitment
    let mut num_commit = 0;
    while num_commit < other_pubkeys.len() {
        let res = rx.recv().await;

        match res {
            Ok(MultiSigMsg::Commitment(pk, c)) if pk != our_keypair.get_public_key() => {
                multi_sig.set_other_commitment(pk, c)?;
                num_commit += 1;
            }
            Ok(_) => {}
            _ => {}
        }
    }

    println!(
        "KP: {} - Received all commit msg!",
        our_keypair.get_public_key()
    );

    /* Reveal stage */

    println!("KP: {} - Start reveal stage", our_keypair.get_public_key());

    // Send our reveal to other keypairs

    tx.send(MultiSigMsg::Reveal(
        our_keypair.get_public_key(),
        multi_sig.get_our_reveal().clone(),
    ))?;

    // Wait for every other reveal

    let mut num_reveal = 0;
    while num_reveal < other_pubkeys.len() {
        let res = rx.recv().await;

        match res {
            Ok(MultiSigMsg::Reveal(pk, r)) if pk != our_keypair.get_public_key() => {
                multi_sig.set_other_reveal(pk, r)?;
                num_reveal += 1;
            }
            Ok(_) => {}
            _ => {}
        }
    }

    println!(
        "KP: {} - Received all reveal msg!",
        our_keypair.get_public_key()
    );

    /* Cosign stage */

    println!("KP: {} - Start cosign stage", our_keypair.get_public_key());

    // Send our cosignature to other keypairs

    tx.send(MultiSigMsg::Cosignature(
        our_keypair.get_public_key(),
        multi_sig.get_our_cosignature(),
    ))?;

    // Wait for every other reveal

    let mut num_cosignature = 0;
    while num_cosignature < other_pubkeys.len() {
        let res = rx.recv().await;

        match res {
            Ok(MultiSigMsg::Cosignature(pk, s)) if pk != our_keypair.get_public_key() => {
                multi_sig.set_other_cosignature(pk, s)?;
                num_cosignature += 1;
            }
            Ok(_) => {}
            _ => {}
        }
    }

    println!(
        "KP: {} - Received all cosign msg!",
        our_keypair.get_public_key()
    );

    /* EVERYONE SIGNED! */

    let signature = multi_sig.musig_cosig.as_ref().unwrap().sign().unwrap();

    match our_keypair.get_public_key() {
        PublicKey::PublicKeyV0(_) => {
            Err(MassaSignatureError::InvalidVersionError(String::from("Wrong PubKey version")).into())
        }
        PublicKey::PublicKeyV1(_pk) => {
            let t = schnorrkel::signing_context(b"massa_sig").bytes(hash.to_bytes());

            let aggregate_pk = multi_sig.musig_cosig.as_ref().unwrap().public_key();

            let result = aggregate_pk.verify(t, &signature);

            result.map_err(|_| MassaSignatureError::MultiSignatureError(String::from("Could not verify the multi-signature")).into())
        }
    }

}

fn original_multi_signature_simulation() {
    let keypairs: Vec<schnorrkel::Keypair> =
        (0..16).map(|_| schnorrkel::Keypair::generate()).collect();

    let t = schnorrkel::signing_context(b"multi-sig").bytes(b"We are legion!");
    let mut commits: Vec<_> = keypairs.iter().map(|k| k.musig(t.clone())).collect();
    for i in 0..commits.len() {
        let r = commits[i].our_commitment();
        for j in commits.iter_mut() {
            assert!(
                j.add_their_commitment(keypairs[i].public, r).is_ok() != (r == j.our_commitment())
            );
        }
    }

    let mut reveal_msgs: Vec<schnorrkel::musig::Reveal> = Vec::with_capacity(commits.len());
    let mut reveals: Vec<_> = commits.drain(..).map(|c| c.reveal_stage()).collect();
    for i in 0..reveals.len() {
        let r = reveals[i].our_reveal().clone();
        for j in reveals.iter_mut() {
            j.add_their_reveal(keypairs[i].public, r.clone()).unwrap();
        }
        reveal_msgs.push(r);
    }
    let pk = reveals[0].public_key();

    let mut cosign_msgs: Vec<schnorrkel::musig::Cosignature> = Vec::with_capacity(reveals.len());
    let mut cosigns: Vec<_> = reveals
        .drain(..)
        .map(|c| {
            assert_eq!(pk, c.public_key());
            c.cosign_stage()
        })
        .collect();
    for i in 0..cosigns.len() {
        assert_eq!(pk, cosigns[i].public_key());
        let r = cosigns[i].our_cosignature();
        for j in cosigns.iter_mut() {
            j.add_their_cosignature(keypairs[i].public, r).unwrap();
        }
        cosign_msgs.push(r);
        assert_eq!(pk, cosigns[i].public_key());
    }

    // let signature = cosigns[0].sign().unwrap();
    let mut c = schnorrkel::musig::collect_cosignatures(t.clone());
    for i in 0..cosigns.len() {
        c.add(keypairs[i].public, reveal_msgs[i].clone(), cosign_msgs[i])
            .unwrap();
    }
    let signature = c.signature();

    assert!(pk.verify(t, &signature).is_ok());
    for cosign in &cosigns {
        assert_eq!(pk, cosign.public_key());
        assert_eq!(signature, cosign.sign().unwrap());
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
                assert_eq!(keypair.a.public, deserialized.a.public);
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

    #[test]
    fn test_multi_signature_simulation() {
        original_multi_signature_simulation();
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 3)]
    async fn test_multi_signature() -> Result<(), Error> {
        start_multi_signature_scheme().await?;

        Ok(())
    }
}
