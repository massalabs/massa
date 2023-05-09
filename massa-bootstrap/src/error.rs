// Copyright (c) 2022 MASSA LABS <info@massa.net>

use std::io::ErrorKind;

use crate::messages::{BootstrapClientMessage, BootstrapServerMessage};
use displaydoc::Display;
use massa_consensus_exports::error::ConsensusError;
use massa_final_state::FinalStateError;
use massa_hash::MassaHashError;
use massa_pos_exports::PosError;
use massa_protocol_exports::ProtocolError;
use massa_serialization::SerializeError;
use massa_time::TimeError;
use thiserror::Error;

#[non_exhaustive]
#[derive(Display, Error, Debug)]
pub(crate)  enum BootstrapError {
    /// all io errors except for Timedout, and would-block (unix error when timed out)
    IoError(std::io::Error),
    /// We want to handle Timedout and WouldBlock
    TimedOut(std::io::Error),
    /// general bootstrap error: {0}
    GeneralError(String),
    /// deserialization error: {0}
    DeserializeError(String),
    /// models error: {0}
    ModelsError(#[from] massa_models::error::ModelsError),
    /// serialize error: {0}
    SerializeError(#[from] SerializeError),
    /// unexpected message received from server: {0:?}
    UnexpectedServerMessage(BootstrapServerMessage),
    /// unexpected message received from client: {0:?}
    UnexpectedClientMessage(Box<BootstrapClientMessage>),
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
    /// protocol error: {0}
    ProtocolError(#[from] ProtocolError),
    /// final state error: {0}
    FinalStateError(#[from] FinalStateError),
    /// Proof-of-Stake error: {0}
    PoSError(#[from] PosError),
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
    /// The bootstrap process ended prematurely - e.g. too much time elapsed
    Interupted(String),
}

/// # Platform-specific behavior
///
/// Platforms may return a different error code whenever a read times out as
/// a result of setting this option. For example Unix typically returns an
/// error of the kind [`ErrorKind::WouldBlock`], but Windows may return [`ErrorKind::TimedOut`].)
impl From<std::io::Error> for BootstrapError {
    fn from(e: std::io::Error) -> Self {
        if e.kind() == ErrorKind::TimedOut || e.kind() == ErrorKind::WouldBlock {
            BootstrapError::TimedOut(e)
        } else {
            BootstrapError::IoError(e)
        }
    }
}
