// Copyright (c) 2022 MASSA LABS <info@massa.net>

use massa_models::{amount::Amount, slot::Slot};

use serde::{Deserialize, Serialize};

/// slot / amount pair
#[derive(Debug, Deserialize, Serialize)]
pub(crate)  struct SlotAmount {
    /// slot
    pub(crate)  slot: Slot,
    /// amount
    pub(crate)  amount: Amount,
}
