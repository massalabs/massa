//! Module caching saves compiled `massa-sc-runtime` modules for later execution.
//! Its purpose is to improve `massa-execution-worker` SC execution performances.
//!
//! See [this discussion](https://github.com/massalabs/massa/discussions/3560#discussioncomment-5190071)
//! for more information about its behaviour.

#![feature(let_chains)]

pub mod config;
pub mod controller;
pub mod error;
mod hd_cache;
mod lru_cache;
pub(crate) mod types;
