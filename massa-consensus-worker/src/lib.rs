// Copyright (c) 2022 MASSA LABS <info@massa.net>

//! # General module description
//!
//! The consensus worker launches a persistent thread that will run in the background.
//! This thread has a `run` function that triggers the consensus algorithm each slot. It can be interrupted by commands
//! that are managed on the fly. The consensus worker share a state with a controller. This controller can be called by the others modules.
//! It avoid sending message to the thread just for getting informations on the consensus.
//!
//! Communications with execution is blocking. Communications with protocol blocks on sending information to protocol but not blocking
//! when protocol sends informations to this module.
//!
//! This module doesn't use asynchronous code.

#![feature(deadline_api)]
#![feature(let_chains)]

mod commands;
mod controller;
mod manager;
mod state;
mod worker;

pub use worker::start_consensus_worker;
