// Copyright (c) 2022 MASSA LABS <info@massa.net>

//! This module exposes useful tooling for testing.
//! It is only compiled and exported by the crate if the "testing" feature is enabled.
//!
//!
//! # Architecture
//!
//! ## config.rs
//! Provides a default execution configuration for testing.
//!
//! ## mock.rs
//! Provides a mock of ExecutionController to simulate interactions
//! with an execution worker within tests.

mod config;
mod mock;

pub use config::*;
pub use mock::*;
