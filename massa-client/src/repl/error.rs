use thiserror::Error;

#[derive(Error, Debug)]
pub enum ReplError {
    #[error("Error during command execution")]
    ExecCommanError,
    #[error("Error during command parsing")]
    ParseCommandError,
    #[error("Error command:{0} not found")]
    CommandNotFoundError(String),
    #[error("PeerInfo channel has disconnected err:{0}")]
    NodeConnectionError(#[from] reqwest::Error),
}
