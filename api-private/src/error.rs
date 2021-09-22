use communication::CommunicationError;
use consensus::ConsensusError;
use crypto::CryptoError;
use models::ModelsError;
use thiserror::Error;
use time::TimeError;

#[derive(Error, Debug)]
pub enum PrivateApiError {
    #[error("send  channel error: {0}")]
    SendChannelError(String),
    #[error("receive  channel error: {0}")]
    ReceiveChannelError(String),
    #[error("crypto error : {0}")]
    CryptoError(#[from] CryptoError),
    #[error("consensus error : {0}")]
    ConsensusError(#[from] ConsensusError),
    #[error("communication error : {0}")]
    CommunicationError(#[from] CommunicationError),
    #[error("models error : {0}")]
    ModelsError(#[from] ModelsError),
    #[error("time error : {0}")]
    TimeError(#[from] TimeError),
    #[error("not found")]
    NotFound,
    #[error("inconsistency: {0}")]
    InconsistencyError(String),
    #[error("missing command sender {0}")]
    MissingCommandSender(String),
    #[error("missing config {0}")]
    MissingConfig(String),
}

impl From<PrivateApiError> for jsonrpc_core::Error {
    fn from(err: PrivateApiError) -> Self {
        jsonrpc_core::Error {
            code: jsonrpc_core::ErrorCode::ServerError(500),
            message: err.to_string(),
            data: None,
        }
    }
}
