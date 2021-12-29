use displaydoc::Display;
use thiserror::Error;

/// Errors of the execution component.
#[non_exhaustive]
#[derive(Display, Error, Debug)]
pub enum ExecutionError {
    /// Channel error
    ChannelError(String),

    /// Join error
    JoinError,

    /// crypto error: {0}
    ModelsError(#[from] massa_models::ModelsError),

    /// time error: {0}
    TimeError(#[from] massa_time::TimeError),

    /// File error
    FileError(String),
}

macro_rules! bootstrap_file_error {
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
pub(crate) use bootstrap_file_error;
