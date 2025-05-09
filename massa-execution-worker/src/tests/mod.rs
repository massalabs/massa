// Copyright (c) 2022 MASSA LABS <info@massa.net>

//! # General description
//!
//! Unit tests for the massa-execution-worker crate
//!
//! # Hierarchy
//!
//! ## interface
//!
//! Unit tests checking for ABI implementation.
//!
//! ## tests_active_history.rs
//!
//! Unit tests for the ActiveHistory struct.
//!
//! ## scenarios_mandatories.rs
//!
//! Complex unit tests using mocks for some parts of the massa node. See universe.rs for more information
//! about the testing framework.
//!
//! # Wasm source code
//!
//! A lot of unit tests requires to deploy some very simple smart contract (ABI tests, SC limit tests...).
//!
//! You can find the wasm source code in the [massa-unit-test-src repository](https://github.com/massalabs/massa-unit-tests-src)
//! and easily modify / compile them.

#[cfg(test)]
mod scenarios_mandatories;

#[cfg(test)]
mod universe;

#[cfg(test)]
mod tests_active_history;

mod interface;
