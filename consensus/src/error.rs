// Copyright (c) 2021 MASSA LABS <info@massa.net>

use crate::consensus_worker::ConsensusEvent;
use communication::CommunicationError;
use models::ModelsError;
use rand::distributions::WeightedError;
use std::array::TryFromSliceError;
use thiserror::Error;

#[derive(Error, Debug)]
pub enum InternalError {
    #[error("transaction error {0}")]
    TransactionError(String),
}

#[derive(Error, Debug)]
pub enum ConsensusError {
    #[error("Our key is missing")]
    KeyError,
    #[error("models error: {0}")]
    ModelsError(#[from] ModelsError),
    #[error("Could not create genesis block {0}")]
    GenesisCreationError(String),
    #[error("Could not propagate block: {0}")]
    WeightedDistributionError(#[from] WeightedError),
    #[error("random selector seed is too short to be safe")]
    SmallSeedError,
    #[error("time overflow")]
    TimeOverflowError,
    #[error("not final roll")]
    NotFinalRollError,
    #[error("roll overflow")]
    RollOverflowError,
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
    #[error("there was an inconsistency between containers {0}")]
    ContainerInconsistency(String),
    #[error("fitness overflow")]
    FitnessOverflow,
    #[error("Send  channel error : {0}")]
    SendChannelError(String),
    #[error("Receive  channel error : {0}")]
    ReceiveChannelError(String),
    #[error("Storage error : {0}")]
    StorageError(#[from] storage::StorageError),
    #[error("pool error : {0}")]
    PoolError(#[from] pool::PoolError),
    #[error("sled error: {0}")]
    SledError(#[from] sled::Error),
    #[error("error reading leger {0}")]
    ReadError(String),
    #[error("try from slice error {0}")]
    TryFromSliceError(#[from] TryFromSliceError),
    #[error("ledger inconsistency error {0}")]
    LedgerInconsistency(String),
    #[error("invalid ledger change: {0}")]
    InvalidLedgerChange(String),
    #[error("invalid roll update: {0}")]
    InvalidRollUpdate(String),
    #[error("sled error {0}")]
    SledTransactionError(#[from] sled::transaction::TransactionError<InternalError>),
    #[error("io error {0}")]
    IOError(#[from] std::io::Error),
    #[error("serde error")]
    SerdeError(#[from] serde_json::Error),
    #[error("oneshot recv error {0}")]
    OneshotReceiveError(#[from] tokio::sync::oneshot::error::RecvError),
    #[error("block creation error {0}")]
    BlockCreationError(String),
    #[error("Proof of stake cycle unavailable {0}")]
    PosCycleUnavailable(String),
    #[error("error sending consensus event: {0}")]
    TokioSendError(#[from] tokio::sync::mpsc::error::SendError<ConsensusEvent>),
    #[error("channel error: {0}")]
    ChannelError(String),
    #[error("amount overflow")]
    AmountOverflowError,
}
