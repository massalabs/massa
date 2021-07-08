use secp256k1;
use failure::{Error, bail};
pub use secp256k1::SecretKey as SecretKey;
pub use secp256k1::PublicKey as PublicKey;
use bs58;

pub trait B58able: Sized {
    type Err;
    fn into_b58check(&self) -> String;
    fn from_b58check(in_str: &String) -> Result<Self, Self::Err>;
}

impl B58able for SecretKey {
    type Err = Error;

    fn into_b58check(&self) -> String {
        bs58::encode(self.serialize().to_vec()).into_string()
    }

    fn from_b58check(in_str: &String) -> Result<SecretKey, Error> {
        let decoded = bs58::decode(in_str).into_vec()?;
        const OUT_LEN:usize = 32;
        if decoded.len() != OUT_LEN {
            bail!("Wrong decoded length");
        }
        let mut out_array: [u8; OUT_LEN]  = [0; OUT_LEN];
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

    fn from_b58check(in_str: &String) -> Result<PublicKey, Error> {
        let decoded = bs58::decode(in_str).into_vec()?;
        const OUT_LEN:usize = 33;
        if decoded.len() != OUT_LEN {
            bail!("Wrong decoded length");
        }
        let mut out_array: [u8; OUT_LEN]  = [0; OUT_LEN];
        out_array.copy_from_slice(decoded.as_slice()); 
        let res = PublicKey::parse_compressed(&out_array)?;
        Ok(res)
    }
}



#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn b58check_from_public_key() {
        let public_key = PublicKey::parse_compressed(&[
            0x02,
            0xc6, 0x6e, 0x7d, 0x89, 0x66, 0xb5, 0xc5, 0x55,
            0xaf, 0x58, 0x05, 0x98, 0x9d, 0xa9, 0xfb, 0xf8,
            0xdb, 0x95, 0xe1, 0x56, 0x31, 0xce, 0x35, 0x8c,
            0x3a, 0x17, 0x10, 0xc9, 0x62, 0x67, 0x90, 0x63,
        ]).unwrap();
        let encoded = public_key.into_b58check();
        assert_eq!(encoded, "ppEBeqRBUTDbd9317cXvhBuDmypZyT3fgkXDmXHnoV9c");
    }

    #[test]
    fn public_key_from_b58check() {
        let expected_public_key = PublicKey::parse_compressed(&[
            0x02,
            0xc6, 0x6e, 0x7d, 0x89, 0x66, 0xb5, 0xc5, 0x55,
            0xaf, 0x58, 0x05, 0x98, 0x9d, 0xa9, 0xfb, 0xf8,
            0xdb, 0x95, 0xe1, 0x56, 0x31, 0xce, 0x35, 0x8c,
            0x3a, 0x17, 0x10, 0xc9, 0x62, 0x67, 0x90, 0x63,
        ]).unwrap();
        let decoded = PublicKey::from_b58check(&String::from("ppEBeqRBUTDbd9317cXvhBuDmypZyT3fgkXDmXHnoV9c")).unwrap();
        assert_eq!(decoded, expected_public_key);
    }
    
    #[test]
    fn b58check_from_secret_key() {
        let secret_key = SecretKey::parse(&[
            0xd3, 0x62, 0xc9, 0xe6, 0xd1, 0x48, 0xbc, 0x20,
            0xf7, 0xee, 0xd8, 0x90, 0x07, 0x80, 0xd5, 0x7f,
            0x47, 0x72, 0xf7, 0xf1, 0x9f, 0x38, 0x4f, 0xd0,
            0x6e, 0x78, 0x7f, 0xf2, 0x2f, 0xac, 0x3d, 0x48,
        ]).unwrap();
        let encoded = secret_key.into_b58check();
        assert_eq!(encoded, "FEAPpWypnp5Gin5zg1QmY4SuhRBxUSNHRUJjozHZEKio");
    }

    #[test]
    fn secret_key_from_b58check() {
        let expected_secret_key = SecretKey::parse(&[
            0xd3, 0x62, 0xc9, 0xe6, 0xd1, 0x48, 0xbc, 0x20,
            0xf7, 0xee, 0xd8, 0x90, 0x07, 0x80, 0xd5, 0x7f,
            0x47, 0x72, 0xf7, 0xf1, 0x9f, 0x38, 0x4f, 0xd0,
            0x6e, 0x78, 0x7f, 0xf2, 0x2f, 0xac, 0x3d, 0x48,
        ]).unwrap();
        let decoded = SecretKey::from_b58check(&String::from("FEAPpWypnp5Gin5zg1QmY4SuhRBxUSNHRUJjozHZEKio")).unwrap();
        assert_eq!(decoded, expected_secret_key);
    }
}
