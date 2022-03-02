use std::{fmt::Display, marker::PhantomData};

use crate::{array_from_slice, DeserializeCompact, ModelsError, SerializeCompact};
use massa_hash::hash::Hash;
use massa_signature::{
    sign, verify_signature, PrivateKey, PublicKey, Signature, SIGNATURE_SIZE_BYTES,
};
use serde::{Deserialize, Serialize};
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Signed<T, U>
where
    T: SerializeCompact + DeserializeCompact + Signable<U> + Display,
    U: Id,
{
    pub content: T,
    pub signature: Signature,
    phantom: PhantomData<U>,
}
pub trait Id {
    fn new(hash: Hash) -> Self;
}

impl<T, U> Display for Signed<T, U>
where
    T: SerializeCompact + DeserializeCompact + Signable<U> + Display,
    U: Id,
{
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        writeln!(f, "Signature: {}", self.signature)?;
        writeln!(f, "{}", self.content)?;
        Ok(())
    }
}
pub trait Signable<U>
where
    U: Id,
    Self: SerializeCompact,
{
    fn get_signature_message(&self) -> Result<Hash, ModelsError>;
    fn compute_id(&self) -> Result<U, ModelsError> {
        Ok(U::new(Hash::compute_from(&self.to_bytes_compact()?)))
    }
}

impl<T, U> Signed<T, U>
where
    T: SerializeCompact + DeserializeCompact + Signable<U> + Display,
    U: Id,
{
    pub fn new_signed(content: T, private_key: &PrivateKey) -> Result<(U, Self), ModelsError> {
        Ok((
            content.compute_id()?,
            Self {
                signature: sign(&content.get_signature_message()?, private_key)?,
                content,
                phantom: PhantomData,
            },
        ))
    }

    pub fn verify_signature(&self, public_key: &PublicKey) -> Result<(), ModelsError> {
        Ok(verify_signature(
            &self.content.get_signature_message()?,
            &self.signature,
            public_key,
        )?)
    }
}

impl<T, U> SerializeCompact for Signed<T, U>
where
    T: SerializeCompact + DeserializeCompact + Signable<U> + Display,
    U: Id,
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

impl<T, U> DeserializeCompact for Signed<T, U>
where
    T: SerializeCompact + DeserializeCompact + Signable<U> + Display,
    U: Id,
{
    fn from_bytes_compact(buffer: &[u8]) -> Result<(Self, usize), ModelsError> {
        let mut cursor = 0usize;

        // signed content
        let (content, delta) = T::from_bytes_compact(&buffer[cursor..])?;
        cursor += delta;

        // signature
        let signature = Signature::from_bytes(&array_from_slice(&buffer[cursor..])?)?;
        cursor += SIGNATURE_SIZE_BYTES;

        Ok((
            Self {
                content,
                signature,
                phantom: PhantomData,
            },
            cursor,
        ))
    }
}
