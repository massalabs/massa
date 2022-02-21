// Copyright (c) 2022 MASSA LABS <info@massa.net>

//! This module exposes useful tooling for testing.
//! It is only compiled and exported by the crate if the "testing" feature is enabled.

mod config;
mod mock;

pub use config::*;
pub use mock::*;
