use thiserror::Error;

#[derive(Error, Debug)]
pub enum ReplError {
    #[error("Error: {0}")]
    GeneralError(String),
    #[error("Error during command parsing")]
    ParseCommandError,
    #[error("Error command: {0} not found")]
    CommandNotFoundError(String),
    #[error("Node connection error err: {0}")]
    NodeConnectionError(#[from] reqwest::Error),
    #[error("Bad input parameter: {0}")]
    BadCommandParameter(String),
    #[error("Error can't create address from specified hash: {0}")]
    AddressCreationError(String),
    #[error("Str Format error: {0}")]
    FmtError(#[from] std::fmt::Error),
    #[error("IO error: {0}")]
    IOError(#[from] std::io::Error),
    #[error("JSON error: {0}")]
    JSONError(#[from] serde_json::Error),
    #[error("Serde Sq error: {0}")]
    SerdeqsError(#[from] serde_qs::Error),
    #[error("wallet error :{0}")]
    WalletError(#[from] wallet::WalletError),
}
