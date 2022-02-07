// Copyright (c) 2021 MASSA LABS <info@massa.net>

use displaydoc::Display;
use thiserror::Error;

#[non_exhaustive]
#[derive(Display, Error, Debug)]
pub enum LedgerError {
    /// there was an inconsistency between containers
    ContainerInconsistency(String),
}
