use flexbuffers::{DeserializationError, ReaderError, SerializationError};
use thiserror::Error;

#[derive(Error, Debug)]
pub enum ModelsError {
    #[error("Header is not hashable")]
    HeaderhashError,
    #[error("conversion error {0}")]
    ConversionError(String),
    #[error("serialization error {0}")]
    SerializationError(#[from] SerializationError),
    #[error("reader error {0}")]
    ReaderError(#[from] ReaderError),
    #[error("deserialization error {0}")]
    DeserializationError(#[from] DeserializationError),
    #[error("slot overflow")]
    SlotOverflowError,
    #[error("thread overflow")]
    ThreadOverflowError,
}
