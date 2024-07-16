// Copyright (c) 2022 MASSA LABS <info@massa.net>

//! this file defines all possible execution error categories

use displaydoc::Display;
use massa_module_cache::error::CacheError;
use massa_sc_runtime::VMError;
use massa_versioning::versioning_factory::FactoryError;
use thiserror::Error;

/// Errors of the execution component.
#[non_exhaustive]
#[derive(Clone, Display, Error, Debug)]
pub enum ExecutionError {
    /// Channel error
    ChannelError(String),

    /// Runtime error: {0}
    RuntimeError(String),

    /// `MassaHashError`: {0}
    MassaHashError(#[from] massa_hash::MassaHashError),

    /// `ModelsError`: {0}
    ModelsError(#[from] massa_models::error::ModelsError),

    /// `RollBuy` error: {0}
    RollBuyError(String),

    /// `RollSell` error: {0}
    RollSellError(String),

    /// Slash roll or deferred credits  error: {0}
    SlashError(String),

    /// `Transaction` error: {0}
    TransactionError(String),

    /// Block gas error: {0}
    BlockGasError(String),

    /// Invalid slot range
    InvalidSlotRange,

    /// Not enough gas in the block: {0}
    NotEnoughGas(String),

    /// Given gas is above the threshold: {0}
    TooMuchGas(String),

    /// Include operation error: {0}
    IncludeOperationError(String),

    /// Include denunciation error: {0}
    IncludeDenunciationError(String),

    /// VM Error in {context} context: {error}
    VMError {
        /// execution context in which the error happened
        context: String,
        /// `massa-sc-runtime` virtual machine error
        error: VMError,
    },

    /// Cache error: {0}
    CacheError(#[from] CacheError),

    /// Factory error: {0}
    FactoryError(#[from] FactoryError),

    /// Autonomous smart contract call error: {0}
    DeferredCallsError(String),
}

/// Execution query errors
#[derive(Clone, Display, Error, Debug)]
pub enum ExecutionQueryError {
    /// Not found: {0}
    NotFound(String),
}
