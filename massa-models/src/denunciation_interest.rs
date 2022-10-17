// Copyright (c) 2022 MASSA LABS <info@massa.net>

use crate::endorsement::WrappedEndorsement;
use crate::operation::WrappedOperation;
use crate::block::WrappedHeader;

/// What can be sent to Denunciation factory (in order to create Denunciation)
#[derive(Debug)]
pub enum DenunciationInterest {
    /// Send a new wrapped endorsement to Denunciation factory
    WrappedEndorsement(WrappedEndorsement),
    /// Send new operations to Denunciation factory (ideally only Operation<Denunciation>)
    WrappedOperations(Vec<WrappedOperation>),
    /// Send new block header to Denunciation factory
    WrappedHeader(WrappedHeader), // Wrapped<BlockHeader, BlockId>
    /// Use to notify for final cs period to Denunciation Factory
    Final(Vec<u64>)
}
