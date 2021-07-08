use bs58;
use failure::{bail, Error};
use secp256k1;
pub use secp256k1::{PublicKey,SecretKey,PublicKeyFormat};
use sha2::{Sha256, Digest};

/// Useful constants
pub const SECRET_KEY_SIZE: usize = secp256k1::util::SECRET_KEY_SIZE;
pub const COMPRESSED_PUBLIC_KEY_SIZE: usize = secp256k1::util::COMPRESSED_PUBLIC_KEY_SIZE;
pub const SIGNATURE_SIZE: usize = secp256k1::util::SIGNATURE_SIZE;


/// Trait representing an object that can be converted to base58
pub trait B58able: Sized {
    type Err;
    /// Convert to base58check string
    fn into_b58check(&self) -> String;
    /// Parse from a base58check string
    fn from_b58check(in_str: &str) -> Result<Self, Self::Err>;
}

impl B58able for SecretKey {
    type Err = Error;

    fn into_b58check(&self) -> String {
        bs58::encode(self.serialize().to_vec()).into_string()
    }

    /// Convert to base58
    ///
    /// # Arguments
    ///
    /// * `in_str` - A string slice that holds the base58check encoding
    ///
    fn from_b58check(in_str: &str) -> Result<SecretKey, Error> {
        let decoded = bs58::decode(in_str).into_vec()?;
        if decoded.len() != SECRET_KEY_SIZE {
            bail!("Wrong decoded length");
        }
        let mut out_array = [0u8; SECRET_KEY_SIZE];
        out_array.copy_from_slice(decoded.as_slice());
        let res = SecretKey::parse(&out_array)?;
        Ok(res)
    }
}

impl B58able for PublicKey {
    type Err = Error;

    fn into_b58check(&self) -> String {
        bs58::encode(self.serialize_compressed().to_vec()).into_string()
    }

    fn from_b58check(in_str: &str) -> Result<PublicKey, Error> {
        let decoded = bs58::decode(in_str).into_vec()?;
        if decoded.len() != COMPRESSED_PUBLIC_KEY_SIZE {
            bail!("Wrong decoded length");
        }
        let mut out_array = [0u8; COMPRESSED_PUBLIC_KEY_SIZE];
        out_array.copy_from_slice(decoded.as_slice());
        let res = PublicKey::parse_compressed(&out_array)?;
        Ok(res)
    }
}

/// Trait representing an object that can sign messages
pub trait SignatureProducer {
    /// Generate message signature
    fn generate_signature(&self, data: &[u8]) -> Result<[u8 ; SIGNATURE_SIZE], Error>;
}

impl SignatureProducer for SecretKey {
    fn generate_signature(&self, data: &[u8]) -> Result<[u8 ; SIGNATURE_SIZE], Error> {
        let mut hasher = Sha256::new();
        hasher.input(data);
        let hashed = hasher.result();
        let msg = secp256k1::Message::parse_slice(&hashed[..])?;
        let (sig, _) = secp256k1::sign(&msg, &self);
        Ok(sig.serialize())
    }
}

/// Trait representing an object that can verify signed messages
pub trait SignatureVerifier {
    /// Verify message signature
    fn verify_signature(&self, data: &[u8], sig: &[u8 ; SIGNATURE_SIZE]) -> Result<(), Error>;
}

impl SignatureVerifier for PublicKey {
    fn verify_signature(&self, data: &[u8], sig: &[u8 ; SIGNATURE_SIZE]) -> Result<(), Error> {
        let mut hasher = Sha256::new();
        hasher.input(data);
        let hashed = hasher.result();
        let msg = secp256k1::Message::parse_slice(&hashed[..])?;
        match secp256k1::verify(&msg, &secp256k1::Signature::parse(sig), &self) {
            true => Ok(()),
            false => bail!("Signature verification failed")
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn b58check_from_public_key() {
        let public_key = PublicKey::parse_compressed(&[
            0x02, 0xc6, 0x6e, 0x7d, 0x89, 0x66, 0xb5, 0xc5, 0x55, 0xaf, 0x58, 0x05, 0x98, 0x9d,
            0xa9, 0xfb, 0xf8, 0xdb, 0x95, 0xe1, 0x56, 0x31, 0xce, 0x35, 0x8c, 0x3a, 0x17, 0x10,
            0xc9, 0x62, 0x67, 0x90, 0x63,
        ])
        .unwrap();
        let encoded = public_key.into_b58check();
        assert_eq!(encoded, "ppEBeqRBUTDbd9317cXvhBuDmypZyT3fgkXDmXHnoV9c");
    }

    #[test]
    fn public_key_from_b58check() {
        let expected_public_key = PublicKey::parse_compressed(&[
            0x02, 0xc6, 0x6e, 0x7d, 0x89, 0x66, 0xb5, 0xc5, 0x55, 0xaf, 0x58, 0x05, 0x98, 0x9d,
            0xa9, 0xfb, 0xf8, 0xdb, 0x95, 0xe1, 0x56, 0x31, 0xce, 0x35, 0x8c, 0x3a, 0x17, 0x10,
            0xc9, 0x62, 0x67, 0x90, 0x63,
        ])
        .unwrap();
        let decoded =
            PublicKey::from_b58check("ppEBeqRBUTDbd9317cXvhBuDmypZyT3fgkXDmXHnoV9c").unwrap();
        assert_eq!(decoded, expected_public_key);
    }

    #[test]
    fn b58check_from_secret_key() {
        let secret_key = SecretKey::parse(&[
            0xd3, 0x62, 0xc9, 0xe6, 0xd1, 0x48, 0xbc, 0x20, 0xf7, 0xee, 0xd8, 0x90, 0x07, 0x80,
            0xd5, 0x7f, 0x47, 0x72, 0xf7, 0xf1, 0x9f, 0x38, 0x4f, 0xd0, 0x6e, 0x78, 0x7f, 0xf2,
            0x2f, 0xac, 0x3d, 0x48,
        ])
        .unwrap();
        let encoded = secret_key.into_b58check();
        assert_eq!(encoded, "FEAPpWypnp5Gin5zg1QmY4SuhRBxUSNHRUJjozHZEKio");
    }

    #[test]
    fn secret_key_from_b58check() {
        let expected_secret_key = SecretKey::parse(&[
            0xd3, 0x62, 0xc9, 0xe6, 0xd1, 0x48, 0xbc, 0x20, 0xf7, 0xee, 0xd8, 0x90, 0x07, 0x80,
            0xd5, 0x7f, 0x47, 0x72, 0xf7, 0xf1, 0x9f, 0x38, 0x4f, 0xd0, 0x6e, 0x78, 0x7f, 0xf2,
            0x2f, 0xac, 0x3d, 0x48,
        ])
        .unwrap();
        let decoded =
            SecretKey::from_b58check("FEAPpWypnp5Gin5zg1QmY4SuhRBxUSNHRUJjozHZEKio").unwrap();
        assert_eq!(decoded, expected_secret_key);
    }
}
