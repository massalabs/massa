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

macro_rules! init_file_error {
    ($st:expr, $cfg:ident) => {
        |err| {
            ExecutionError::FileError(format!(
                "error $st initial SCE ledger file {}: {}",
                $cfg.settings
                    .initial_sce_ledger_path
                    .to_str()
                    .unwrap_or("(non-utf8 path)"),
                err
            ))
        }
    };
}
pub(crate) use init_file_error;
