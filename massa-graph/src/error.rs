use std::array::TryFromSliceError;

// Copyright (c) 2022 MASSA LABS <info@massa.net>
use displaydoc::Display;
use massa_execution_exports::ExecutionError;
use massa_models::ModelsError;
use massa_proof_of_stake_exports::error::ProofOfStakeError;
use thiserror::Error;

/// Result used in the graph
pub type GraphResult<T, E = GraphError> = core::result::Result<T, E>;

/// Result used in the ledger
pub type LedgerResult<T, E = LedgerError> = core::result::Result<T, E>;

#[non_exhaustive]
#[derive(Display, Error, Debug)]
pub enum GraphError {
    /// execution error: {0}
    ExecutionError(#[from] ExecutionError),
    /// models error: {0}
    ModelsError(#[from] ModelsError),
    /// Could not create genesis block {0}
    GenesisCreationError(String),
    /// missing block
    MissingBlock,
    /// there was an inconsistency between containers {0}
    ContainerInconsistency(String),
    /// fitness overflow
    FitnessOverflow,
    /// invalid ledger change: {0}
    InvalidLedgerChange(String),
    /// io error {0}
    IOError(#[from] std::io::Error),
    /// serde error
    SerdeError(#[from] serde_json::Error),
    /// Proof of Stake error {0}
    ProofOfStakeError(#[from] ProofOfStakeError),
    /// Proof of stake cycle unavailable {0}
    PosCycleUnavailable(String),
    /// Ledger error {0}
    LedgerError(#[from] LedgerError),
    /// transaction error {0}
    TransactionError(String),
}

#[non_exhaustive]
#[derive(Display, Error, Debug)]
pub enum InternalError {
    /// transaction error {0}
    TransactionError(String),
}

#[non_exhaustive]
#[derive(Display, Error, Debug)]
pub enum LedgerError {
    /// amount overflow
    AmountOverflowError,
    /// ledger inconsistency error {0}
    LedgerInconsistency(String),
    /// sled error: {0}
    SledError(#[from] sled::Error),
    /// sled error {0}
    SledTransactionError(#[from] sled::transaction::TransactionError<InternalError>),
    /// models error: {0}
    ModelsError(#[from] ModelsError),
    /// try from slice error {0}
    TryFromSliceError(#[from] TryFromSliceError),
    /// io error {0}
    IOError(#[from] std::io::Error),
    /// serde error
    SerdeError(#[from] serde_json::Error),
}
