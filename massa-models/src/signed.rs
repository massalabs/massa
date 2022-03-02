use crate::{array_from_slice, DeserializeCompact, ModelsError, SerializeCompact};
use massa_hash::hash::Hash;
use massa_signature::{
    sign, verify_signature, PrivateKey, PublicKey, Signature, SIGNATURE_SIZE_BYTES,
};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Signed<T: SerializeCompact + DeserializeCompact + Signable> {
    pub content: T,
    pub signature: Signature,
}

pub trait Signable {
    fn get_signature_message(&self) -> Hash;
}

impl<T> Signed<T>
where
    T: SerializeCompact + DeserializeCompact + Signable,
{
    pub fn new_signed(content: T, private_key: &PrivateKey) -> Result<Self, ModelsError> {
        Ok(Self {
            signature: sign(&content.get_signature_message(), private_key)?,
            content,
        })
    }

    pub fn verify_signature(&self, public_key: &PublicKey) -> Result<(), ModelsError> {
        Ok(verify_signature(
            &self.content.get_signature_message(),
            &self.signature,
            public_key,
        )?)
    }
}

impl<T> SerializeCompact for Signed<T>
where
    T: SerializeCompact + DeserializeCompact + Signable,
{
    fn to_bytes_compact(&self) -> Result<Vec<u8>, ModelsError> {
        let mut res: Vec<u8> = Vec::new();

        // signed content
        res.extend(self.content.to_bytes_compact()?);

        // signature
        res.extend(&self.signature.to_bytes());

        Ok(res)
    }
}

impl<T> DeserializeCompact for Signed<T>
where
    T: SerializeCompact + DeserializeCompact + Signable,
{
    fn from_bytes_compact(buffer: &[u8]) -> Result<(Self, usize), ModelsError> {
        let mut cursor = 0usize;

        // signed content
        let (content, delta) = T::from_bytes_compact(&buffer[cursor..])?;
        cursor += delta;

        // signature
        let signature = Signature::from_bytes(&array_from_slice(&buffer[cursor..])?)?;
        cursor += SIGNATURE_SIZE_BYTES;

        Ok((Self { content, signature }, cursor))
    }
}
