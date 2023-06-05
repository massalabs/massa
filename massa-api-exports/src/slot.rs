// Copyright (c) 2022 MASSA LABS <info@massa.net>

use massa_models::{amount::Amount, slot::Slot};

use serde::{Deserialize, Serialize};

/// slot / amount pair
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct SlotAmount {
    /// slot
    pub slot: Slot,
    /// amount
    pub amount: Amount,
}
