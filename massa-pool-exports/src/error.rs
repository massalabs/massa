//! Copyright (c) 2022 MASSA LABS <info@massa.net>

use displaydoc::Display;
use massa_models::ModelsError;
use thiserror::Error;

/// pool error
#[non_exhaustive]
#[derive(Display, Error, Debug)]
pub enum PoolError {
    /// there was an inconsistency between containers
    ContainerInconsistency(String),
    /// channel error : {0}
    ChannelError(String),
    /// models error: {0}
    ModelsError(#[from] ModelsError),
    /// missing operation error: {0}
    MissingOperation(String),
}
