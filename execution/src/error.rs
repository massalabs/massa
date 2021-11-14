use displaydoc::Display;
use thiserror::Error;

/// Errors of the execution component.
#[derive(Display, Error, Debug)]
pub enum ExecutionError {
    /// Empty for now.
    Nothing,
}
