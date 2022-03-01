// Copyright (c) 2022 MASSA LABS <info@massa.net>

use crate::messages::BootstrapMessage;
use displaydoc::Display;
use massa_consensus_exports::error::ConsensusError;
use massa_hash::MassaHashError;
use massa_ledger::LedgerError;
use massa_network::NetworkError;
use massa_time::TimeError;
use thiserror::Error;

#[non_exhaustive]
#[derive(Display, Error, Debug)]
pub enum BootstrapError {
    /// io error: {0}
    IoError(#[from] std::io::Error),
    /// general bootstrap error: {0}
    GeneralError(String),
    /// models error: {0}
    ModelsError(#[from] massa_models::ModelsError),
    /// unexpected message from bootstrap node: {0:?}
    UnexpectedMessage(BootstrapMessage),
    /// connection with bootstrap node dropped
    UnexpectedConnectionDrop,
    /// massa_hash error: {0}
    MassaHashError(#[from] MassaHashError),
    /// time error: {0}
    TimeError(#[from] TimeError),
    /// consensus error: {0}
    ConsensusError(#[from] ConsensusError),
    /// network error: {0}
    NetworkError(#[from] NetworkError),
    /// ledger error: {0}
    LedgerError(#[from] LedgerError),
    /// join error: {0}
    JoinError(#[from] tokio::task::JoinError),
    /// missing private key file
    MissingKeyError,
    /// incompatible version: {0}
    IncompatibleVersionError(String),
}
