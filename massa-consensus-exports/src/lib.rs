// Copyright (c) 2022 MASSA LABS <info@massa.net>

#![feature(async_closure)]
#![feature(bool_to_option)]
#![feature(hash_drain_filter)]
#![feature(map_first_last)]
#![feature(int_roundings)]

#[macro_use]
extern crate massa_logging;

pub use consensus_controller::{ConsensusCommandSender, ConsensusEventReceiver, ConsensusManager};
pub use error::ConsensusError;
use massa_models::{Address, Slot};
pub use settings::{ConsensusConfig, ConsensusSettings};

mod consensus_controller;
pub mod error;
pub mod settings;

pub mod commands;

pub mod events;

// Usefull defined types
type SelectionDraws = Vec<(Slot, (Address, Vec<Address>))>;

pub mod tools;
