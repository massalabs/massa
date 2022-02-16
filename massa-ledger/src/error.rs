// Copyright (c) 2021 MASSA LABS <info@massa.net>

use displaydoc::Display;
use thiserror::Error;

#[non_exhaustive]
#[derive(Display, Error, Debug)]
pub enum LedgerError {
    /// container iconsistency: {0}
    ContainerInconsistency(String),
    /// missing entry: {0}
    MissingEntry(String),
}
