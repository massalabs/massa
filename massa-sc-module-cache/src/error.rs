use displaydoc::Display;
use thiserror::Error;
// use massa_serialization::{DeserializeError, SerializeError};

/// Cache result
pub type CacheResult<T, E = CacheError> = core::result::Result<T, E>;

/// Cache error
#[non_exhaustive]
#[derive(Display, Error, Debug)]
pub enum CacheError {
    /// VM error: {0}
    VMError(String),
}

impl From<anyhow::Error> for CacheError {
    fn from(e: anyhow::Error) -> CacheError {
        return CacheError::VMError(e.to_string());
    }
}
