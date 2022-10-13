// Copyright (c) 2022 MASSA LABS <info@massa.net>

use crate::endorsement::WrappedEndorsement;
use crate::operation::WrappedOperation;
use crate::block::{BlockHeader, WrappedHeader};

#[derive(Debug)]
pub enum DenunciationInterest {
    WrappedEndorsement(WrappedEndorsement),
    WrappedOperations(Vec<WrappedOperation>),
    WrappedHeader(WrappedHeader), // Wrapped<BlockHeader, BlockId>
}
