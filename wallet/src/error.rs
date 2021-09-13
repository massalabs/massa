use thiserror::Error;

// todo remove display
#[derive(Error, Debug)]
pub enum WalletError {
    #[error("IO error: {0}")]
    IOError(#[from] std::io::Error),
    #[error("JSON error: {0}")]
    JSONError(#[from] serde_json::Error),
    #[error("Serde Sq error: {0}")]
    SerdeqsError(#[from] serde_qs::Error),
    #[error("Models error: {0}")]
    ModelsError(#[from] models::ModelsError),
}
