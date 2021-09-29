use communication::CommunicationError;
use consensus::ConsensusError;
use crypto::CryptoError;
use displaydoc::Display;
use models::ModelsError;
use thiserror::Error;
use time::TimeError;

#[non_exhaustive]
#[derive(Display, Error, Debug)]
pub enum PrivateApiError {
    /// send  channel error: {0}
    SendChannelError(String),
    /// receive  channel error: {0}
    ReceiveChannelError(String),
    /// crypto error : {0}
    CryptoError(#[from] CryptoError),
    /// consensus error : {0}
    ConsensusError(#[from] ConsensusError),
    /// communication error : {0}
    CommunicationError(#[from] CommunicationError),
    /// models error : {0}
    ModelsError(#[from] ModelsError),
    /// time error : {0}
    TimeError(#[from] TimeError),
    /// not found
    NotFound,
    /// inconsistency: {0}
    InconsistencyError(String),
    /// missing command sender {0}
    MissingCommandSender(String),
    /// missing config {0}
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
