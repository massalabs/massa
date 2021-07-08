use communication::CommunicationError;
use models::error::ModelsError;
use rand::distributions::WeightedError;
use thiserror::Error;

#[derive(Error, Debug)]
pub enum ConsensusError {
    #[error("Our key is missing")]
    KeyError,
    #[error("Could not hash block header")]
    HeaderHashError(#[from] ModelsError),
    #[error("Could not create genesis block")]
    GenesisCreationError,
    #[error("Could not propagate block")]
    WeightedDistributionError(#[from] WeightedError),
    #[error("random selector seed is too short to be safe")]
    SmallSeedError,
    #[error("time overflow")]
    TimeOverflowError,
    #[error("slot overflow")]
    SlotOverflowError,
    #[error("thread overflow")]
    ThreadOverflowError,
    #[error("hash conversion error")]
    HashConversionError,
    #[error("config error")]
    ConfigError(String),
    #[error("crypto error {0}")]
    CryptoError(#[from] crypto::CryptoError),
    #[error("Communication error {0}")]
    CommunicationError(#[from] CommunicationError),
    #[error("failed retrieving consensus controller event")]
    ControllerEventError,
    #[error("Join error {0}")]
    JoinError(#[from] tokio::task::JoinError),
    #[error("Time error {0}")]
    TimeError(#[from] time::TimeError),
}
