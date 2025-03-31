//! Main node configuration and all that stuff is here
//!
//! # Introduction
//!
//! This module is mainly used to define the default values used through
//! the project in all *Configuration* objects.
//!
//! The name "constant" is a used for some hard-coded default values. It shouldn't
//! be used as it as a constant. When you need one of these values, you better
//! use the `cfg` parameter in your worker.
//!
//! The only place where it's safe to use it is in files named `settings.rs`
//! or `config.rs`
//!
//! # test-exports or default
//!
//! You have access to the constants (in test or in normal mode) with the
//! following imports. When you have a doubt for the import, use the auto
//! dispatcher `use massa_models::constants::*;`
//!
//! ```ignore
//! // Classic import automatically root to `test-exports` or `default`
//! // depending on the compilation context.
//! use massa_models::constants::*;
//!
//! // Force to import the nominal constants
//! use massa_models::constants::default::*;
//!
//! // Force to import the test-exports constants
//! use massa_models::constants::default_test-exports::*;
//! ```
//!
//! # Note about rooting
//! ```md
//! |T|S| not(T) or S | T and not(S)
//! |0|0| 1           | 0
//! |1|0| 0           | 1
//! |0|1| 1           | 0
//! |1|1| 1           | 0
//! ```
//!
//! `#[cfg(any(not(feature = "test-exports"), feature = "sandbox"))]`
//! On `cargo run --release` or `cargo run --features sandbox`
//!
//! #`[cfg(all(feature = "test-exports", not(feature = "sandbox")))]`
//! On `cargo run` or `cargo test`
//!

// **************************************************************
// Note:
// We can force the access to one of defined value (test or not)
// with `use massa_config::exported_constants::CONST_VALUE`
// Nevertheless the design is more like using `massa_config::CONST_VALUE`
// and defining in `Cargo.toml` if we are test-exports or not
// ```toml
// [dependencies]
//     massa_config = { path = "../massa-config" }
// [dev-dependencies]
//     massa_config = { path = "../massa-config", features = ["test-exports"] }
// ```
//

pub mod constants;
pub use constants::*;

mod compact_config;
pub use compact_config::CompactConfig;

// Export tool to read user setting file
mod massa_settings;
pub use massa_settings::build_massa_settings;

mod massa_disclaimer;
pub use massa_disclaimer::handle_disclaimer;
