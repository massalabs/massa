// Copyright (c) 2021 MASSA LABS <info@massa.net>

use crate::consensus_worker::ConsensusEvent;
use displaydoc::Display;
use models::ModelsError;
use protocol_exports::ProtocolError;
use rand::distributions::WeightedError;
use std::array::TryFromSliceError;
use thiserror::Error;

#[non_exhaustive]
#[derive(Display, Error, Debug)]
pub enum InternalError {
    /// transaction error {0}
    TransactionError(String),
}

#[non_exhaustive]
#[derive(Display, Error, Debug)]
pub enum ConsensusError {
    /// Our key is missing
    KeyError,
    /// models error: {0}
    ModelsError(#[from] ModelsError),
    /// Could not create genesis block {0}
    GenesisCreationError(String),
    /// Could not propagate block: {0}
    WeightedDistributionError(#[from] WeightedError),
    /// random selector seed is too short to be safe
    SmallSeedError,
    /// time overflow
    TimeOverflowError,
    /// not final roll
    NotFinalRollError,
    /// roll overflow
    RollOverflowError,
    /// slot overflow
    SlotOverflowError,
    /// thread overflow
    ThreadOverflowError,
    /// hash conversion error
    HashConversionError,
    /// config error: {0}
    ConfigError(String),
    /// massa_hash error {0}
    MassaHashError(#[from] massa_hash::MassaHashError),
    /// Protocol error {0}
    ProtocolError(#[from] ProtocolError),
    /// failed retrieving consensus controller event
    ControllerEventError,
    /// Join error {0}
    JoinError(#[from] tokio::task::JoinError),
    /// Time error {0}
    TimeError(#[from] time::TimeError),
    /// invalid block
    InvalidBlock,
    /// missing block
    MissingBlock,
    /// there was an inconsistency between containers {0}
    ContainerInconsistency(String),
    /// fitness overflow
    FitnessOverflow,
    /// Send  channel error : {0}
    SendChannelError(String),
    /// Receive  channel error : {0}
    ReceiveChannelError(String),
    /// pool error : {0}
    PoolError(#[from] pool::PoolError),
    /// sled error: {0}
    SledError(#[from] sled::Error),
    /// error reading leger {0}
    ReadError(String),
    /// try from slice error {0}
    TryFromSliceError(#[from] TryFromSliceError),
    /// ledger inconsistency error {0}
    LedgerInconsistency(String),
    /// invalid ledger change: {0}
    InvalidLedgerChange(String),
    /// invalid roll update: {0}
    InvalidRollUpdate(String),
    /// sled error {0}
    SledTransactionError(#[from] sled::transaction::TransactionError<InternalError>),
    /// io error {0}
    IOError(#[from] std::io::Error),
    /// serde error
    SerdeError(#[from] serde_json::Error),
    /// oneshot recv error {0}
    OneshotReceiveError(#[from] tokio::sync::oneshot::error::RecvError),
    /// block creation error {0}
    BlockCreationError(String),
    /// Proof of stake cycle unavailable {0}
    PosCycleUnavailable(String),
    /// error sending consensus event: {0}
    TokioSendError(#[from] tokio::sync::mpsc::error::SendError<ConsensusEvent>),
    /// channel error: {0}
    ChannelError(String),
    /// amount overflow
    AmountOverflowError,
}
