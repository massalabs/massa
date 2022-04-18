use std::{fmt::Display, marker::PhantomData};

use crate::{array_from_slice, DeserializeCompact, ModelsError, SerializeCompact};
use massa_hash::Hash;
use massa_signature::{
    sign, verify_signature, PrivateKey, PublicKey, Signature, SIGNATURE_SIZE_BYTES,
};
use serde::{Deserialize, Serialize};

/// Signed structure T where U is the associated id
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Signed<T, U>
where
    T: SerializeCompact + DeserializeCompact + Signable<U> + Display,
    U: Id,
{
    /// content
    pub content: T,
    /// signature
    pub signature: Signature,
    #[serde(skip)]
    phantom: PhantomData<U>,
}

/// Used by signed structure
pub trait Id {
    /// new id from hash
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

/// implement if you want that structure to be signed
pub trait Signable<U>
where
    U: Id,
    Self: SerializeCompact,
{
    /// The hash that should be used for the signature
    fn get_signature_message(&self) -> Result<Hash, ModelsError> {
        Ok(Hash::compute_from(&self.to_bytes_compact()?))
    }

    /// Associated id
    fn compute_id(&self) -> Result<U, ModelsError> {
        Ok(U::new(Hash::compute_from(&self.to_bytes_compact()?)))
    }
}

impl<T, U> Signed<T, U>
where
    T: SerializeCompact + DeserializeCompact + Signable<U> + Display,
    U: Id,
{
    /// generate new signed structure and id
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

    /// check if self has been signed by public key
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
