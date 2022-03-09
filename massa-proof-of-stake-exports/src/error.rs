// Copyright (c) 2022 MASSA LABS <info@massa.net>
use displaydoc::Display;
use massa_models::ModelsError;
use thiserror::Error;

pub type POSResult<T, E = ProofOfStakeError> = core::result::Result<T, E>;

#[non_exhaustive]
#[derive(Display, Error, Debug)]
pub enum ProofOfStakeError {
    /// Proof of stake cycle unavailable {0}
    PosCycleUnavailable(String),
    /// there was an inconsistency between containers {0}
    ContainerInconsistency(String),
    /// not final roll
    NotFinalRollError,
    /// roll overflow
    RollOverflowError,
    /// io error {0}
    IOError(#[from] std::io::Error),
    /// serde error
    SerdeError(#[from] serde_json::Error),
    /// models error: {0}
    ModelsError(#[from] ModelsError),
}
