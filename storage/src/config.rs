// Copyright (c) 2021 MASSA LABS <info@massa.net>

use serde::Deserialize;
use std::path::PathBuf;
use time::UTime;

#[derive(Debug, Deserialize, Clone)]
pub struct StorageConfig {
    /// Max number of blocks we want to store
    pub max_stored_blocks: usize,
    /// path to db
    pub path: PathBuf,
    pub cache_capacity: u64,
    pub flush_interval: Option<UTime>,
    pub reset_at_startup: bool,
    pub max_item_return_count: usize,
    pub disable_storage: bool,
}
