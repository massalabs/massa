//! Copyright (c) 2022 MASSA LABS <info@massa.net>

//! Pool of operation and endorsements waiting to be included in a block

#![warn(missing_docs)]
#![warn(unused_crate_dependencies)]
#![feature(map_first_last)]
#![feature(async_closure)]
#![feature(map_try_insert)]

mod controller_impl;
mod endorsement_pool;
mod operation_pool;
mod protection;
mod run;
mod types;

pub use run::start_pool;
#[cfg(test)]
pub use run::start_pool_without_protection;

#[cfg(test)]
mod tests;
