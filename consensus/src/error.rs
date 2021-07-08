use communication::CommunicationError;
use crypto::hash::Hash;
use models::block::Block;
use models::error::ModelsError;
use rand::distributions::WeightedError;
use std::collections::HashSet;
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
    ContainerInconsistency,
    #[error("fitness overflow")]
    FitnessOverflow,
    #[error("Send  channel error : {0}")]
    SendChannelError(String),
    #[error("Receive  channel error : {0}")]
    ReceiveChannelError(String),
    #[error("Storage error : {0}")]
    StorageError(#[from] storage::error::StorageError),
}

#[derive(Error, Debug)]
pub enum BlockAcknowledgeError {
    #[error("crypto error {0}")]
    CryptoError(#[from] crypto::CryptoError),
    #[error("time error {0}")]
    TimeError(#[from] time::TimeError),
    #[error("consensus error {0}")]
    ConsensusError(#[from] ConsensusError),
    #[error("wrong signature")]
    WrongSignature,
    #[error("block was previously discarded")]
    AlreadyDiscarded,
    #[error("block was previously acknowledged")]
    AlreadyAcknowledged,
    #[error("block has invalid fields")]
    InvalidFields,
    #[error("block is too old")]
    TooOld,
    #[error("block is in the future")]
    InTheFuture(Block),
    #[error("block is too much in the future")]
    TooMuchInTheFuture,
    #[error("it wasn't the block creator's turn to create a block in this slot")]
    DrawMismatch,
    #[error("block's parents are invalid or badly chosen {0}")]
    InvalidParents(String),
    #[error("block verification requires for dependencies")]
    MissingDependencies(Block, HashSet<Hash>),
    #[error("Container Inconsistency")]
    ContainerInconsistency,
}
