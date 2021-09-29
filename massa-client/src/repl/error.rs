use displaydoc::Display;
use thiserror::Error;

#[non_exhaustive]
#[derive(Display, Error, Debug)]
pub enum ReplError {
    /// Error: {0}
    GeneralError(String),
    /// Error during command parsing
    ParseCommandError,
    /// Error command: {0} not found
    CommandNotFoundError(String),
    /// Node connection error err: {0}
    NodeConnectionError(#[from] reqwest::Error),
    /// Bad input parameter: {0}
    BadCommandParameter(String),
    /// Error can't create address from specified hash: {0}
    AddressCreationError(String),
    /// Str Format error: {0}
    FmtError(#[from] std::fmt::Error),
    /// IO error: {0}
    IOError(#[from] std::io::Error),
    /// JSON error: {0}
    JSONError(#[from] serde_json::Error),
    /// Serde Sq error: {0}
    SerdeqsError(#[from] serde_qs::Error),
    /// wallet error :{0}
    WalletError(#[from] wallet::WalletError),
}
