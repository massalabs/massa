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
//!
//! ## Workflow description
//!
//! The consensus worker is launched and initializes a shared state that contains caches, counters and other info.
//! The consensus worker wakes up at each slot or when it receives a command.
//! If a command is received, the worker executes it and goes back to sleep.
//!  * When an incoming block header is fed to the module, it registers it and asks the Protocol module for the dependencies of the block (including the full block itself) if needed.
//!  * When an incoming full block is fed to the module, it registers it and asks the Protocol module for its dependencies if needed.
//!    * If the dependencies are already available, the module checks if it can validate the block and add it to a clique.
//!    * If it's the second block received for the same slot we save it in order to denounce the creator in the future.
//!    * If it's the third or more we ignore the block unless we asked for it explicitly as a dependency.
//! If a queued block reaches the slot time at which it should be processed, the worker wakes up to check it and trigger, if necessary, the consensus algorithm.
//! It then prunes the block graph and the caches.

#![feature(deadline_api)]
#![feature(let_chains)]

mod commands;
mod controller;
mod manager;
mod state;
mod worker;

pub(crate)  use worker::start_consensus_worker;
