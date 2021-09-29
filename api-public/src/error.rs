use communication::CommunicationError;
use consensus::ConsensusError;
use crypto::CryptoError;
use displaydoc::Display;
use models::ModelsError;
use pool::PoolError;
use storage::StorageError;
use thiserror::Error;
use time::TimeError;

#[non_exhaustive]
#[derive(Display, Error, Debug)]
pub enum PublicApiError {
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
    /// pool error : {0}
    PoolError(#[from] PoolError),
    /// storage error : {0}
    StorageError(#[from] StorageError),
    /// not found
    NotFound,
    /// inconsistency: {0}
    InconsistencyError(String),
    /// missing command sender {0}
    MissingCommandSender(String),
    /// missing config {0}
    MissingConfig(String),
}

impl From<PublicApiError> for jsonrpc_core::Error {
    fn from(err: PublicApiError) -> Self {
        jsonrpc_core::Error {
            code: jsonrpc_core::ErrorCode::ServerError(500),
            message: err.to_string(),
            data: None,
        }
    }
}
