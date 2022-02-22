// Copyright (c) 2022 MASSA LABS <info@massa.net>

//!
//!
//!
//!
//!
//!
//! TODO algo description
//!
//!
//!
//!
//!

#![feature(map_first_last)]
#![feature(unzip_option)]

mod context;
mod controller;
mod execution;
mod interface_impl;
mod speculative_ledger;
mod vm_thread;

pub use vm_thread::start_execution_worker;

#[cfg(test)]
mod tests;
