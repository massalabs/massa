use bs58;
use flexbuffers;

use bitcoin_hashes::sha256;
use secp256k1;
use secp256k1::rand::rngs::OsRng;
use secp256k1::{Message, Secp256k1};

#[derive(Debug)]
pub enum SignatureError {
    ParseError,
}

impl std::error::Error for SignatureError {}

impl std::fmt::Display for SignatureError {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        match self {
            SignatureError::ParseError => write!(f, "Parse  Error"),
        }
    }
}

/// Private Key used to sign messages
/// Generated using SignatureEngine.
#[derive(Copy, Clone, Debug, Ord, PartialOrd, Eq, PartialEq)]
pub struct PrivateKey(secp256k1::SecretKey);

impl PrivateKey {
    /// Serialize a PrivateKey using bs58 encoding with checksum.
    ///
    /// # Example
    ///  ```
    /// let mut rng = secp256k1::rand::rngs::OsRng::new().expect("OsRng");
    /// let private_key = SignatureEngine::generate_random_private_key(&mut rng);
    ///
    /// let serialized: String = private_key.serialize_bs58_check();
    /// ```
    pub fn serialize_bs58_check(&self) -> String {
        bs58::encode(self.serialize_binary())
            .with_check()
            .into_string()
    }

    /// Serialize a PrivateKey as bytes.
    ///
    /// # Example
    ///  ```
    /// let mut rng = secp256k1::rand::rngs::OsRng::new().expect("OsRng");
    /// let private_key = SignatureEngine::generate_random_private_key(&mut rng);
    ///
    /// let serialized : Vec<u8> = private_key.serialize_binary();
    /// ```
    pub fn serialize_binary(&self) -> Vec<u8> {
        self.0.as_ref().to_vec()
    }

    /// Deserialize a PrivateKey using bs58 encoding with checksum.
    ///
    /// # Example
    ///  ```
    /// let mut rng = secp256k1::rand::rngs::OsRng::new().expect("OsRng");
    /// let private_key = SignatureEngine::generate_random_private_key(&mut rng);
    ///
    /// let serialized: String = private_key.serialize_bs58_check();
    /// let deserialized: PrivateKey = PrivateKey::deserialize_bs58_check(&serialized).unwrap();
    /// ```
    pub fn deserialize_bs58_check(data: &str) -> Result<PrivateKey, SignatureError> {
        match bs58::decode(data).with_check(None).into_vec() {
            Ok(s) => Ok(PrivateKey::deserialize_binary(&s)?),
            _ => Err(SignatureError::ParseError),
        }
    }

    /// Deserialize a PrivateKey from bytes.
    ///
    /// # Example
    ///  ```
    /// let mut rng = secp256k1::rand::rngs::OsRng::new().expect("OsRng");
    /// let private_key = SignatureEngine::generate_random_private_key(&mut rng);
    ///
    /// let serialized : Vec<u8> = private_key.serialize_binary();
    /// let deserialized: PrivateKey = PrivateKey::deserialize_binary(&serialized).unwrap();
    /// ```
    pub fn deserialize_binary(data: &Vec<u8>) -> Result<PrivateKey, SignatureError> {
        match secp256k1::key::SecretKey::from_slice(&data[..]) {
            Ok(k) => Ok(PrivateKey(k)),
            _ => Err(SignatureError::ParseError),
        }
    }
}

impl ::serde::Serialize for PrivateKey {
    /// ::serde::Serialize trait for PrivateKey
    /// if the serializer is human readable,
    /// serialization is done using serialize_bs58_check
    /// else, it uses serialize_binary
    ///
    /// # Example
    ///
    /// Human readable serialization :
    /// ```
    /// let mut rng = secp256k1::rand::rngs::OsRng::new().expect("OsRng");
    /// let private_key = SignatureEngine::generate_random_private_key(&mut rng);
    ///
    /// let serialized: String = serde_json::to_string(&private_key).unwrap();
    /// ```
    ///
    /// Not human readable serialization :
    /// ```
    /// let mut rng = secp256k1::rand::rngs::OsRng::new().expect("OsRng");
    /// let private_key = SignatureEngine::generate_random_private_key(&mut rng);
    ///
    /// let mut s = flexbuffers::FlexbufferSerializer::new();
    /// private_key.serialize(&mut s).unwrap();
    /// let serialized: Vec<u8> = s.view().to_vec();
    /// ```
    fn serialize<S: ::serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        if s.is_human_readable() {
            s.collect_str(&self.serialize_bs58_check())
        } else {
            s.serialize_bytes(&self.serialize_binary())
        }
    }
}

impl<'de> ::serde::Deserialize<'de> for PrivateKey {
    /// ::serde::Deserialize trait for PrivateKey
    /// if the deserializer is human readable,
    /// deserialization is done using deserialize_bs58_check
    /// else, it uses deserialize_binary
    ///
    /// # Example
    ///
    /// Human readable deserialization :
    /// ```
    /// let mut rng = secp256k1::rand::rngs::OsRng::new().expect("OsRng");
    /// let private_key = SignatureEngine::generate_random_private_key(&mut rng);
    ///
    /// let serialized = serde_json::to_string(&private_key).unwrap();
    /// let deserialized: PrivateKey = serde_json::from_str(&serialized).unwrap();
    /// ```
    ///
    /// Not human readable deserialization :
    /// ```
    /// let mut rng = secp256k1::rand::rngs::OsRng::new().expect("OsRng");
    /// let private_key = SignatureEngine::generate_random_private_key(&mut rng);
    ///
    /// let mut s = flexbuffers::FlexbufferSerializer::new();
    /// private_key.serialize(&mut s).unwrap();
    ///
    /// let r = flexbuffers::Reader::get_root(s.view()).unwrap();
    /// let deserialized: PrivateKey = PrivateKey::deserialize(r).unwrap();
    /// ```
    fn deserialize<D: ::serde::Deserializer<'de>>(d: D) -> Result<PrivateKey, D::Error> {
        if d.is_human_readable() {
            struct Base58CheckVisitor;

            impl<'de> ::serde::de::Visitor<'de> for Base58CheckVisitor {
                type Value = PrivateKey;

                fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
                    formatter.write_str("an ASCII base58check string")
                }

                fn visit_bytes<E>(self, v: &[u8]) -> Result<Self::Value, E>
                where
                    E: ::serde::de::Error,
                {
                    if let Ok(v_str) = std::str::from_utf8(v) {
                        PrivateKey::deserialize_bs58_check(&v_str).map_err(E::custom)
                    } else {
                        Err(E::invalid_value(::serde::de::Unexpected::Bytes(v), &self))
                    }
                }

                fn visit_str<E>(self, v: &str) -> Result<Self::Value, E>
                where
                    E: ::serde::de::Error,
                {
                    PrivateKey::deserialize_bs58_check(&v).map_err(E::custom)
                }
            }
            d.deserialize_str(Base58CheckVisitor)
        } else {
            struct BytesVisitor;

            impl<'de> ::serde::de::Visitor<'de> for BytesVisitor {
                type Value = PrivateKey;

                fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
                    formatter.write_str("a bytestring")
                }

                fn visit_bytes<E>(self, v: &[u8]) -> Result<Self::Value, E>
                where
                    E: ::serde::de::Error,
                {
                    PrivateKey::deserialize_binary(&v.to_vec()).map_err(E::custom)
                }
            }

            d.deserialize_bytes(BytesVisitor)
        }
    }
}

/// Public key used to check if a message was encoded
/// by the corresponding PublicKey.
/// Generated from the PrivateKey using SignatureEngine
#[derive(Copy, Clone, Debug, Ord, PartialOrd, Eq, PartialEq, Hash)]
pub struct PublicKey(secp256k1::PublicKey);

impl PublicKey {
    /// Serialize a PublicKey using bs58 encoding with checksum.
    ///
    /// # Example
    ///  ```
    /// let secp = SignatureEngine::new();
    /// let mut rng = secp256k1::rand::rngs::OsRng::new().expect("OsRng");
    /// let private_key = SignatureEngine::generate_random_private_key(&mut rng);
    /// let public_key = secp.derive_public_key(&private_key);
    ///
    /// let serialized: String = public_key.serialize_bs58_check();
    /// ```
    pub fn serialize_bs58_check(&self) -> String {
        bs58::encode(self.serialize_binary())
            .with_check()
            .into_string()
    }

    /// Serialize a PublicKey as bytes.
    ///
    /// # Example
    ///  ```
    /// let secp = SignatureEngine::new();
    /// let mut rng = secp256k1::rand::rngs::OsRng::new().expect("OsRng");
    /// let private_key = SignatureEngine::generate_random_private_key(&mut rng);
    /// let public_key = secp.derive_public_key(&private_key);
    ///
    /// let serialized: Vec<u8> = public_key.serialize_binary();
    /// ```
    pub fn serialize_binary(&self) -> Vec<u8> {
        self.0.serialize().to_vec()
    }

    /// Deserialize a PublicKey using bs58 encoding with checksum.
    ///
    /// # Example
    ///  ```
    /// let secp = SignatureEngine::new();
    /// let mut rng = secp256k1::rand::rngs::OsRng::new().expect("OsRng");
    /// let private_key = SignatureEngine::generate_random_private_key(&mut rng);
    /// let public_key = secp.derive_public_key(&private_key);
    ///
    /// let serialized: String = public_key.serialize_bs58_check();
    /// let deserialized: PublicKey = PublicKey::deserialize_bs58_check(&serialized).unwrap();
    /// ```
    pub fn deserialize_bs58_check(data: &str) -> Result<PublicKey, SignatureError> {
        match bs58::decode(data).with_check(None).into_vec() {
            Ok(s) => Ok(PublicKey::deserialize_binary(&s)?),
            _ => Err(SignatureError::ParseError),
        }
    }

    /// Deserialize a PublicKey from bytes.
    ///
    /// # Example
    ///  ```
    /// let mut rng = secp256k1::rand::rngs::OsRng::new().expect("OsRng");
    /// let secp = SignatureEngine::new();
    /// let private_key = SignatureEngine::generate_random_private_key(&mut rng);
    /// let public_key = secp.derive_public_key(&private_key);
    ///
    /// let serialized : Vec<u8> = public_key.serialize_binary();
    /// let deserialized: PublicKey = PublicKey::deserialize_binary(&serialized).unwrap();
    /// ```
    pub fn deserialize_binary(data: &Vec<u8>) -> Result<PublicKey, SignatureError> {
        match secp256k1::key::PublicKey::from_slice(&data[..]) {
            Ok(k) => Ok(PublicKey(k)),
            _ => Err(SignatureError::ParseError),
        }
    }
}

impl ::serde::Serialize for PublicKey {
    /// ::serde::Serialize trait for PublicKey
    /// if the serializer is human readable,
    /// serialization is done using serialize_bs58_check
    /// else, it uses serialize_binary
    ///
    /// # Example
    ///
    /// Human readable serialization :
    /// ```
    /// let secp = SignatureEngine::new();
    /// let mut rng = secp256k1::rand::rngs::OsRng::new().expect("OsRng");
    /// let private_key = SignatureEngine::generate_random_private_key(&mut rng);
    /// let public_key = secp.derive_public_key(&private_key);
    ///
    /// let serialized: String = serde_json::to_string(&public_key).unwrap();
    /// ```
    ///
    /// Not human readable serialization :
    /// ```
    /// let secp = SignatureEngine::new();
    /// let mut rng = secp256k1::rand::rngs::OsRng::new().expect("OsRng");
    /// let private_key = SignatureEngine::generate_random_private_key(&mut rng);
    /// let public_key = secp.derive_public_key(&private_key);
    ///
    /// let mut s = flexbuffers::FlexbufferSerializer::new();
    /// public_key.serialize(&mut s).unwrap();
    /// let serialized: Vec<u8> = s.view().to_vec();
    /// ```
    fn serialize<S: ::serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        if s.is_human_readable() {
            s.collect_str(&self.serialize_bs58_check())
        } else {
            s.serialize_bytes(&self.serialize_binary())
        }
    }
}

impl<'de> ::serde::Deserialize<'de> for PublicKey {
    /// ::serde::Deserialize trait for PublicKey
    /// if the deserializer is human readable,
    /// deserialization is done using deserialize_bs58_check
    /// else, it uses deserialize_binary
    ///
    /// # Example
    ///
    /// Human readable deserialization :
    /// ```
    /// let mut rng = secp256k1::rand::rngs::OsRng::new().expect("OsRng");
    /// let secp = SignatureEngine::new();
    /// let private_key = SignatureEngine::generate_random_private_key(&mut rng);
    /// let public_key = secp.derive_public_key(&private_key);
    ///
    /// let serialized = serde_json::to_string(&public_key).unwrap();
    /// let deserialized: PublicKey = serde_json::from_str(&serialized).unwrap();
    /// ```
    ///
    /// Not human readable deserialization :
    /// ```
    /// let secp = SignatureEngine::new();
    /// let mut rng = secp256k1::rand::rngs::OsRng::new().expect("OsRng");
    /// let private_key = SignatureEngine::generate_random_private_key(&mut rng);
    /// let public_key = secp.derive_public_key(&private_key);
    ///
    /// let mut s = flexbuffers::FlexbufferSerializer::new();
    /// public_key.serialize(&mut s).unwrap();
    ///
    /// let r = flexbuffers::Reader::get_root(s.view()).unwrap();
    /// let deserialized: PublicKey = PublicKey::deserialize(r).unwrap();
    /// ```
    fn deserialize<D: ::serde::Deserializer<'de>>(d: D) -> Result<PublicKey, D::Error> {
        if d.is_human_readable() {
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
                        PublicKey::deserialize_bs58_check(&v_str).map_err(E::custom)
                    } else {
                        Err(E::invalid_value(::serde::de::Unexpected::Bytes(v), &self))
                    }
                }

                fn visit_str<E>(self, v: &str) -> Result<Self::Value, E>
                where
                    E: ::serde::de::Error,
                {
                    PublicKey::deserialize_bs58_check(&v).map_err(E::custom)
                }
            }
            d.deserialize_str(Base58CheckVisitor)
        } else {
            struct BytesVisitor;

            impl<'de> ::serde::de::Visitor<'de> for BytesVisitor {
                type Value = PublicKey;

                fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
                    formatter.write_str("a bytestring")
                }

                fn visit_bytes<E>(self, v: &[u8]) -> Result<Self::Value, E>
                where
                    E: ::serde::de::Error,
                {
                    PublicKey::deserialize_binary(&v.to_vec()).map_err(E::custom)
                }
            }

            d.deserialize_bytes(BytesVisitor)
        }
    }
}

/// Signature generated from a message and a privateKey.
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub struct Signature(secp256k1::Signature);

impl Signature {
    /// Serialize a Signature using bs58 encoding with checksum.
    ///
    /// # Example
    ///  ```
    /// let mut rng = secp256k1::rand::rngs::OsRng::new().expect("OsRng");
    /// let secp = SignatureEngine::new();
    /// let private_key = SignatureEngine::generate_random_private_key(&mut rng);
    /// let data = "Hello World!".as_bytes().to_vec();
    /// let signature = secp.sign(&data, &private_key);
    ///
    /// let serialized: String = signature.serialize_bs58_check();
    /// ```
    pub fn serialize_bs58_check(&self) -> String {
        bs58::encode(self.serialize_binary())
            .with_check()
            .into_string()
    }

    /// Serialize a Signature as bytes.
    ///
    /// # Example
    ///  ```
    /// let mut rng = secp256k1::rand::rngs::OsRng::new().expect("OsRng");
    /// let secp = SignatureEngine::new();
    /// let private_key = SignatureEngine::generate_random_private_key(&mut rng);
    /// let data = "Hello World!".as_bytes().to_vec();
    /// let signature = secp.sign(&data, &private_key);
    ///
    /// let serialized : Vec<u8> = signature.serialize_binary();
    /// ```
    pub fn serialize_binary(&self) -> Vec<u8> {
        self.0.serialize_compact().to_vec()
    }

    /// Deserialize a Signature using bs58 encoding with checksum.
    ///
    /// # Example
    ///  ```
    /// let mut rng = secp256k1::rand::rngs::OsRng::new().expect("OsRng");
    /// let private_key = SignatureEngine::generate_random_private_key(&mut rng);
    /// let data = "Hello World!".as_bytes().to_vec();
    /// let secp = SignatureEngine::new();
    /// let signature = secp.sign(&data, &private_key);
    ///
    /// let serialized: String = signature.serialize_bs58_check();
    /// let deserialized: Signature = Signature::deserialize_bs58_check(&serialized).unwrap();
    /// ```
    pub fn deserialize_bs58_check(data: &str) -> Result<Signature, SignatureError> {
        match bs58::decode(data).with_check(None).into_vec() {
            Ok(s) => Ok(Signature::deserialize_binary(&s)?),
            _ => Err(SignatureError::ParseError),
        }
    }

    /// Deserialize a Signature from bytes.
    ///
    /// # Example
    ///  ```
    /// let mut rng = secp256k1::rand::rngs::OsRng::new().expect("OsRng");
    /// let secp = SignatureEngine::new();
    /// let private_key = SignatureEngine::generate_random_private_key(&mut rng);
    /// let data = "Hello World!".as_bytes().to_vec();
    /// let signature = secp.sign(&data, &private_key);
    ///
    /// let serialized: Vec<u8> = signature.serialize_binary();
    /// let deserialized: Signature = Signature::deserialize_binary(&serialized).unwrap();
    /// ```
    pub fn deserialize_binary(data: &Vec<u8>) -> Result<Signature, SignatureError> {
        match secp256k1::Signature::from_compact(&data[..]) {
            Ok(k) => Ok(Signature(k)),
            _ => Err(SignatureError::ParseError),
        }
    }
}

impl ::serde::Serialize for Signature {
    /// ::serde::Serialize trait for Signature
    /// if the serializer is human readable,
    /// serialization is done using serialize_bs58_check
    /// else, it uses serialize_binary
    ///
    /// # Example
    ///
    /// Human readable serialization :
    /// ```
    /// let mut rng = secp256k1::rand::rngs::OsRng::new().expect("OsRng");
    /// let secp = SignatureEngine::new();
    /// let private_key = SignatureEngine::generate_random_private_key(&mut rng);
    /// let data = "Hello World!".as_bytes().to_vec();
    /// let signature = secp.sign(&data, &private_key);
    ///
    /// let serialized: String = serde_json::to_string(&signature).unwrap();
    /// ```
    ///
    /// Not human readable serialization :
    /// ```
    /// let secp = SignatureEngine::new();
    /// let mut rng = secp256k1::rand::rngs::OsRng::new().expect("OsRng");
    /// let private_key = SignatureEngine::generate_random_private_key(&mut rng);
    /// let data = "Hello World!".as_bytes().to_vec();
    /// let signature = secp.sign(&data, &private_key);
    ///
    /// let mut s = flexbuffers::FlexbufferSerializer::new();
    /// signature.serialize(&mut s).unwrap();
    /// let serialized: Vec<u8> = s.view().to_vec();
    /// ```
    fn serialize<S: ::serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        if s.is_human_readable() {
            s.collect_str(&self.serialize_bs58_check())
        } else {
            s.serialize_bytes(&self.serialize_binary())
        }
    }
}

impl<'de> ::serde::Deserialize<'de> for Signature {
    /// ::serde::Deserialize trait for Signature
    /// if the deserializer is human readable,
    /// deserialization is done using deserialize_bs58_check
    /// else, it uses deserialize_binary
    ///
    /// # Example
    ///
    /// Human readable deserialization :
    /// ```
    /// let mut rng = secp256k1::rand::rngs::OsRng::new().expect("OsRng");
    /// let secp = SignatureEngine::new();
    /// let private_key = SignatureEngine::generate_random_private_key(&mut rng);
    /// let data = "Hello World!".as_bytes().to_vec();
    /// let signature = secp.sign(&data, &private_key);
    ///
    /// let serialized = serde_json::to_string(&signature).unwrap();
    /// let deserialized: Signature = serde_json::from_str(&serialized).unwrap();
    /// ```
    ///
    /// Not human readable deserialization :
    /// ```
    /// let secp = SignatureEngine::new();
    /// let mut rng = secp256k1::rand::rngs::OsRng::new().expect("OsRng");
    /// let private_key = SignatureEngine::generate_random_private_key(&mut rng);
    /// let data = "Hello World!".as_bytes().to_vec();
    /// let signature = secp.sign(&data, &private_key);
    ///
    /// let mut s = flexbuffers::FlexbufferSerializer::new();
    /// signature.serialize(&mut s).unwrap();
    ///
    /// let r = flexbuffers::Reader::get_root(s.view()).unwrap();
    /// let deserialized: Signature = Signature::deserialize(r).unwrap();
    /// ```
    fn deserialize<D: ::serde::Deserializer<'de>>(d: D) -> Result<Signature, D::Error> {
        if d.is_human_readable() {
            struct Base58CheckVisitor;

            impl<'de> ::serde::de::Visitor<'de> for Base58CheckVisitor {
                type Value = Signature;

                fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
                    formatter.write_str("an ASCII base58check string")
                }

                fn visit_bytes<E>(self, v: &[u8]) -> Result<Self::Value, E>
                where
                    E: ::serde::de::Error,
                {
                    if let Ok(v_str) = std::str::from_utf8(v) {
                        Signature::deserialize_bs58_check(&v_str).map_err(E::custom)
                    } else {
                        Err(E::invalid_value(::serde::de::Unexpected::Bytes(v), &self))
                    }
                }

                fn visit_str<E>(self, v: &str) -> Result<Self::Value, E>
                where
                    E: ::serde::de::Error,
                {
                    Signature::deserialize_bs58_check(&v).map_err(E::custom)
                }
            }
            d.deserialize_str(Base58CheckVisitor)
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
                    Signature::deserialize_binary(&v.to_vec()).map_err(E::custom)
                }
            }

            d.deserialize_bytes(BytesVisitor)
        }
    }
}

/// SignatureEngine manages Key generation,
/// signing and verification.
/// It contains the needed context.
pub struct SignatureEngine(secp256k1::Secp256k1<secp256k1::All>);

impl SignatureEngine {
    /// Generate a new SignatureEngine.
    ///
    /// # Example
    /// ```
    /// let secp: SignatureEngine = SignatureEngine::new();
    /// ```
    pub fn new() -> SignatureEngine {
        SignatureEngine(Secp256k1::new())
    }

    /// Generate a random private key from a RNG.
    /// # Example
    /// ```
    /// let mut rng = secp256k1::rand::rngs::OsRng::new().expect("OsRng");
    /// let private_key: PrivateKey = SignatureEngine::generate_random_private_key(&mut rng);
    /// ```
    pub fn generate_random_private_key<R: rand::Rng + ?Sized>(rng: &mut R) -> PrivateKey {
        PrivateKey(secp256k1::key::SecretKey::new(rng))
    }

    /// Derives a PublicKey from a PrivateKey.
    ///
    /// # Example
    /// ```
    /// let mut rng = secp256k1::rand::rngs::OsRng::new().expect("OsRng");
    /// let secp = SignatureEngine::new();
    /// let private_key = SignatureEngine::generate_random_private_key(&mut rng);
    /// let public_key: PublicKey = secp.derive_public_key(&private_key);
    /// ```
    pub fn derive_public_key(&self, private_key: &PrivateKey) -> PublicKey {
        PublicKey(secp256k1::key::PublicKey::from_secret_key(
            &self.0,
            &private_key.0,
        ))
    }

    /// Returns the Signature produced by signing
    /// data bytes with a PrivateKey.
    ///
    /// # Example
    ///  ```
    /// let mut rng = secp256k1::rand::rngs::OsRng::new().expect("OsRng");
    /// let secp = SignatureEngine::new();
    /// let private_key = SignatureEngine::generate_random_private_key(&mut rng);
    /// let public_key: PublicKey = secp.derive_public_key(&private_key);
    /// let data = "Hello World!".as_bytes().to_vec();
    /// let signature: Signature = secp.sign(&data, &private_key);
    /// ```
    pub fn sign(&self, data: &Vec<u8>, private_key: &PrivateKey) -> Signature {
        let message = Message::from_hashed_data::<sha256::Hash>(data);
        Signature(self.0.sign(&message, &private_key.0))
    }

    /// Checks if the Signature associated with data bytes
    /// was produced with the PrivateKey associated to given PublicKey
    ///
    /// # Example
    ///  ```
    /// let mut rng = secp256k1::rand::rngs::OsRng::new().expect("OsRng");
    /// let secp = SignatureEngine::new();
    /// let private_key = SignatureEngine::generate_random_private_key(&mut rng);
    /// let public_key: PublicKey = secp.derive_public_key(&private_key);
    /// let data = "Hello World!".as_bytes().to_vec();
    /// let signature = secp.sign(&data, &private_key);
    /// let verification: bool = secp.verify(&data, &signature, &public_key);
    /// ```
    pub fn verify(&self, data: &Vec<u8>, signature: &Signature, public_key: &PublicKey) -> bool {
        let message = Message::from_hashed_data::<sha256::Hash>(data);
        self.0.verify(&message, &signature.0, &public_key.0).is_ok()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde::{Deserialize, Serialize};

    #[test]
    fn test_secp256k1_crate_usage() {
        use bitcoin_hashes::sha256;
        use secp256k1::rand::rngs::OsRng;
        use secp256k1::{Message, Secp256k1};

        let secp = Secp256k1::new();
        let mut rng = OsRng::new().expect("OsRng");
        let (secret_key, public_key) = secp.generate_keypair(&mut rng);
        let message = Message::from_hashed_data::<sha256::Hash>("Hello World!".as_bytes());
        let sig = secp.sign(&message, &secret_key);
        assert!(secp.verify(&message, &sig, &public_key).is_ok());
    }

    #[test]
    fn test_example() {
        let secp = SignatureEngine::new();
        let mut rng = OsRng::new().expect("OsRng");
        let private_key = SignatureEngine::generate_random_private_key(&mut rng);
        let public_key = secp.derive_public_key(&private_key);
        let message = "Hello World!".as_bytes().to_vec();
        let signature = secp.sign(&message, &private_key);
        assert!(secp.verify(&message, &signature, &public_key))
    }

    #[test]
    fn test_serde_private_key() {
        let mut rng = OsRng::new().expect("OsRng");
        let private_key = SignatureEngine::generate_random_private_key(&mut rng);
        let serialized =
            serde_json::to_string(&private_key).expect("Could not serialize private key");
        let deserialized =
            serde_json::from_str(&serialized).expect("could not deserialize private key");
        assert_eq!(private_key, deserialized);
    }

    #[test]
    fn test_serde_public_key() {
        let secp = SignatureEngine::new();
        let mut rng = OsRng::new().expect("OsRng");
        let private_key = SignatureEngine::generate_random_private_key(&mut rng);
        let public_key = secp.derive_public_key(&private_key);
        let serialized =
            serde_json::to_string(&public_key).expect("Could not serialize public key");
        let deserialized =
            serde_json::from_str(&serialized).expect("could not deserialize public key");
        assert_eq!(public_key, deserialized);
    }

    #[test]
    fn test_serde_signature() {
        let secp = SignatureEngine::new();
        let mut rng = OsRng::new().expect("OsRng");
        let private_key = SignatureEngine::generate_random_private_key(&mut rng);
        let data = "Hello World!".as_bytes().to_vec();
        let signature = secp.sign(&data, &private_key);
        let serialized =
            serde_json::to_string(&signature).expect("Could not serialize signature key");
        let deserialized =
            serde_json::from_str(&serialized).expect("could not deserialize signature key");
        assert_eq!(signature, deserialized);
    }

    #[test]
    fn test_serde_buf_public_key() {
        let mut s = flexbuffers::FlexbufferSerializer::new();
        let secp = SignatureEngine::new();
        let mut rng = OsRng::new().expect("OsRng");
        let private_key = SignatureEngine::generate_random_private_key(&mut rng);
        let public_key = secp.derive_public_key(&private_key);
        public_key
            .serialize(&mut s)
            .expect("could not serialize public key");
        let r = flexbuffers::Reader::get_root(s.view()).expect("could not read buffer");
        let deserialized = PublicKey::deserialize(r).expect("could not deserialize public key");
        assert_eq!(public_key, deserialized);
    }

    #[test]
    fn test_serde_buf_signature() {
        let mut s = flexbuffers::FlexbufferSerializer::new();
        let secp = SignatureEngine::new();
        let mut rng = OsRng::new().expect("OsRng");
        let private_key = SignatureEngine::generate_random_private_key(&mut rng);
        let data = "Hello World!".as_bytes().to_vec();
        let signature = secp.sign(&data, &private_key);
        signature
            .serialize(&mut s)
            .expect("could not serialize signature key");
        let r = flexbuffers::Reader::get_root(s.view()).expect("could not read buffer");
        let deserialized = Signature::deserialize(r).expect("could not deserialize signature key");
        assert_eq!(signature, deserialized);
    }

    #[test]
    fn test_serde_buf_private_key() {
        let mut s = flexbuffers::FlexbufferSerializer::new();
        let mut rng = OsRng::new().expect("OsRng");
        let private_key = SignatureEngine::generate_random_private_key(&mut rng);
        private_key
            .serialize(&mut s)
            .expect("could not serialize private key");
        let r = flexbuffers::Reader::get_root(s.view()).expect("could not read buffer");
        let deserialized = PrivateKey::deserialize(r).expect("could not deserialize private key");
        assert_eq!(private_key, deserialized);
    }
}
