//! Copyright (c) 2022 MASSA LABS <info@massa.net>

//! This file defines all error types for the async message pool system

use displaydoc::Display;
use thiserror::Error;

#[non_exhaustive]
#[derive(Display, Error, Debug)]
pub enum AsyncPoolError {
    /// container iconsistency: {0}
    ContainerInconsistency(String),
}
