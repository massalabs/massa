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
        bs58::encode(self.serialize().to_vec()).into_string()
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
