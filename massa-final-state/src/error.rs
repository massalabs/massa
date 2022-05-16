//! Copyright (c) 2022 MASSA LABS <info@massa.net>

//! This file defines all error types for final state management

use displaydoc::Display;
use thiserror::Error;

/// Final state error
#[non_exhaustive]
#[derive(Display, Error, Debug)]
pub enum FinalStateError {
    /// ledger error: {0}
    LedgerError(String),
}
