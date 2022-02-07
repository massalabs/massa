// Copyright (c) 2021 MASSA LABS <info@massa.net>

use serde::{Deserialize, Serialize};

/// Ledger configuration
#[derive(Debug, Deserialize, Serialize, Clone, Copy)]
pub struct LedgerConfig {
    pub final_history_length: usize,
}
