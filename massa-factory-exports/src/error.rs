use displaydoc::Display;
use thiserror::Error;

/// factory result
pub(crate)  type FactoryResult<T, E = FactoryError> = core::result::Result<T, E>;

/// factory error
#[non_exhaustive]
#[derive(Display, Error, Debug)]
pub(crate)  enum FactoryError {
    /// Generic error: {0}
    GenericError(String),
}
