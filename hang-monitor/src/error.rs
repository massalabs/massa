use thiserror::Error;

#[derive(Error, Debug)]
pub enum HangMonitorError {
    #[error("Channel error : {0}")]
    ChannelError(String),
    #[error("Join error {0}")]
    JoinError(#[from] tokio::task::JoinError),
}
