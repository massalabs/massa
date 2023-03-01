// Copyright (c) 2022 MASSA LABS <info@massa.net>

use crate::messages::{BootstrapClientMessage, BootstrapServerMessage};
use displaydoc::Display;
use massa_consensus_exports::error::ConsensusError;
use massa_final_state::FinalStateError;
use massa_hash::MassaHashError;
use massa_network_exports::NetworkError;
use massa_pos_exports::PosError;
use massa_serialization::SerializeError;
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
    ModelsError(#[from] massa_models::error::ModelsError),
    /// serialize error: {0}
    SerializeError(#[from] SerializeError),
    /// unexpected message received from server: {0:?}
    UnexpectedServerMessage(BootstrapServerMessage),
    /// unexpected message received from client: {0:?}
    UnexpectedClientMessage(BootstrapClientMessage),
    /// connection with bootstrap node dropped
    UnexpectedConnectionDrop,
    /// `massa_hash` error: {0}
    MassaHashError(#[from] MassaHashError),
    /// `massa_consensus` error: {0}
    MassaConsensusError(#[from] ConsensusError),
    /// `massa_signature` error {0}
    MassaSignatureError(#[from] massa_signature::MassaSignatureError),
    /// time error: {0}
    TimeError(#[from] TimeError),
    /// network error: {0}
    NetworkError(#[from] NetworkError),
    /// final state error: {0}
    FinalStateError(#[from] FinalStateError),
    /// Proof-of-Stake error: {0}
    PoSError(#[from] PosError),
    /// join error: {0}
    JoinError(#[from] tokio::task::JoinError),
    /// missing keypair file
    MissingKeyError,
    /// incompatible version: {0}
    IncompatibleVersionError(String),
    /// Received error: {0}
    ReceivedError(String),
    /// clock error: {0}
    ClockError(String),
    /// fail to init the list from file : {0}
    InitListError(String),
    /// IP {0} is blacklisted
    BlackListed(String),
    /// IP {0} is not in the whitelist
    WhiteListed(String),
}

impl BootstrapError {
    /// Updates the err-msg payload of
    pub fn update_io_msg_payload(self, new_msg: &str) -> Self {
        if let Self::IoError(e) = &self {
            return Self::IoError(std::io::Error::new(e.kind(), new_msg));
        }
        self
    }
}
