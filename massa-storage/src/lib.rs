//! Copyright (c) 2022 MASSA LABS <info@massa.net>

//! This crate is used to share objects (blocks, operations...) across the node

#![warn(missing_docs)]

mod module_storage_usage;
mod storage;

pub use module_storage_usage::ModuleStorageUsage;
pub use storage::Storage;
