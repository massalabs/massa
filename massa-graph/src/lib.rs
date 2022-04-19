// Copyright (c) 2022 MASSA LABS <info@massa.net>
//! graph management
#![warn(missing_docs)]
#![warn(unused_crate_dependencies)]
#![feature(async_closure)]
#![feature(bool_to_option)]
#![feature(hash_drain_filter)]
#![feature(map_first_last)]
#![feature(int_roundings)]

extern crate massa_logging;

pub use settings::LedgerConfig;

/// useful structures
pub mod export_active_block;

mod bootstrapable_graph;
pub use bootstrapable_graph::BootstrapableGraph;

mod block_graph;
pub use block_graph::*;

/// parallel ledger (TODO remove after unification)
pub mod ledger;

/// graph errors
pub mod error;

/// graph settings
pub mod settings;
