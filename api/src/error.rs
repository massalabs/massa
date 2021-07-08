use thiserror::Error;

#[derive(Error, Debug)]
pub enum ApiError {
    #[error("join error {0}")]
    JoinError(#[from] tokio::task::JoinError),
    #[error("send  channel error : {0}")]
    SendChannelError(String),
    #[error("receive  channel error : {0}")]
    ReceiveChannelError(String),
}
