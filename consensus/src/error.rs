use communication::CommunicationError;
use models::ModelsError;
use rand::distributions::WeightedError;
use thiserror::Error;

#[derive(Error, Debug)]
pub enum ConsensusError {
    #[error("Our key is missing")]
    KeyError,
    #[error("Could not hash block header: {0}")]
    HeaderHashError(#[from] ModelsError),
    #[error("Could not create genesis block")]
    GenesisCreationError,
    #[error("Could not propagate block: {0}")]
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
    #[error("config error: {0}")]
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
    #[error("invalid block")]
    InvalidBlock,
    #[error("missing block")]
    MissingBlock,
    #[error("there was an inconsistency between containers")]
    ContainerInconsistency(String),
    #[error("fitness overflow")]
    FitnessOverflow,
    #[error("Send  channel error : {0}")]
    SendChannelError(String),
    #[error("Receive  channel error : {0}")]
    ReceiveChannelError(String),
    #[error("Storage error : {0}")]
    StorageError(#[from] storage::StorageError),
}
