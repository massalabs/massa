//! Copyright (c) 2022 MASSA LABS <info@massa.net>
//! All the structures that are used everywhere
//!
#![warn(missing_docs)]
#![warn(unused_crate_dependencies)]
#![feature(bound_map)]
#![feature(int_roundings)]
#![feature(iter_intersperse)]

use crate::page::PageRequest;
use massa_time::MassaTime;
use serde::{Deserialize, Serialize};

/// address related structures
pub(crate)  mod address;
/// block-related structures
pub(crate)  mod block;
/// node configuration
pub(crate)  mod config;
/// datastore serialization / deserialization
pub(crate)  mod datastore;
/// endorsements
pub(crate)  mod endorsement;
/// models error
pub(crate)  mod error;
/// execution
pub(crate)  mod execution;
/// ledger structures
pub(crate)  mod ledger;
/// node related structure
pub(crate)  mod node;
/// operations
pub(crate)  mod operation;
/// page
pub(crate)  mod page;
/// rolls
pub(crate)  mod rolls;
/// slots
pub(crate)  mod slot;

/// Dumb utils function to display nicely boolean value
fn display_if_true(value: bool, text: &str) -> String {
    if value {
        format!("[{}]", text)
    } else {
        String::from("")
    }
}

/// Help to format Optional bool
fn display_option_bool(
    value: Option<bool>,
    text_true: &str,
    text_false: &str,
    text_none: &str,
) -> String {
    match value {
        Some(true) => {
            format!("[{}]", text_true)
        }
        Some(false) => {
            format!("[{}]", text_false)
        }
        None => {
            format!("[{}]", text_none)
        }
    }
}

/// Just a wrapper with a optional beginning and end
#[derive(Debug, Deserialize, Clone, Copy, Serialize)]
pub(crate)  struct TimeInterval {
    /// optional start slot
    pub(crate)  start: Option<MassaTime>,
    /// optional end slot
    pub(crate)  end: Option<MassaTime>,
}

/// SCRUD operations
#[derive(strum::Display)]
#[strum(serialize_all = "snake_case")]
pub(crate)  enum ScrudOperation {
    /// search operation
    Search,
    /// create operation
    Create,
    /// read operation
    Read,
    /// update operation
    Update,
    /// delete operation
    Delete,
}

/// Bootstrap lists types
#[derive(strum::Display)]
#[strum(serialize_all = "snake_case")]
pub(crate)  enum ListType {
    /// contains banned entry
    Blacklist,
    /// contains allowed entry
    Whitelist,
}

/// Wrap request params into struct for ApiV2 method
#[derive(Deserialize, Serialize)]
pub(crate)  struct ApiRequest {
    /// pagination
    pub(crate)  page_request: Option<PageRequest>,
}
