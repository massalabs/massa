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
//! The consensus worker is launched and initialize the shared state that contains a lot of caches, counters and informations.
//! The consensus worker wake up himself each slot or when it receives a command.
//! If he received a command, he will execute it and then go back to sleep.
//!  * In case of adding a block header, it will register it and ask to protocol for dependencies if needed (him included).
//!  * In case of adding a block, it will register it and ask to protocol for dependencies if needed.
//!    * If it already have the dependencies, it will check if it can validate the block and add it to a clique. 
//!    * If it's the second block received for the same slot we will save it for denunciate the creator in the future.
//!    * It it's the third or mode we ignore the block.
//! If he reached the slot time then it will run some checks and trigger, if necessary, the consensus algorithm.
//! It also prune the block graph and the caches.

#![feature(deadline_api)]
#![feature(let_chains)]

mod commands;
mod controller;
mod manager;
mod state;
mod worker;

pub use worker::start_consensus_worker;
