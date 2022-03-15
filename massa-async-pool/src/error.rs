// Copyright (c) 2022 MASSA LABS <info@massa.net>

//! This file defines all error types for the ledger system

use displaydoc::Display;
use thiserror::Error;

#[non_exhaustive]
#[derive(Display, Error, Debug)]
pub enum LedgerError {
    /// container iconsistency: {0}
    ContainerInconsistency(String),
    /// missing entry: {0}
    MissingEntry(String),
    /// file error: {0}
    FileError(String),
}
