use displaydoc::Display;
use thiserror::Error;
// use massa_serialization::{DeserializeError, SerializeError};

/// Cache error
#[non_exhaustive]
#[derive(Display, Error, Debug, Clone)]
pub enum CacheError {
    /// VM error: {0}
    VMError(String),
    /// Load error: {0}
    LoadError(String),
}

impl From<anyhow::Error> for CacheError {
    fn from(e: anyhow::Error) -> CacheError {
        return CacheError::VMError(e.to_string());
    }
}
