use crate::amount::Amount;
use crate::slot::Slot;
use serde::{Deserialize, Serialize};

/// Represent a vesting range
#[derive(Clone, Copy, Deserialize, Serialize, Debug)]
pub struct VestingRange {
    /// start slot of range
    /// use "slot" field in the initial_vesting.json file
    #[serde(rename(deserialize = "slot", serialize = "slot"))]
    pub start_slot: Slot,

    /// end slot for the range
    /// Init with 0,0 and calculate on load
    #[serde(default = "init_end_slot_range")]
    #[serde(skip_serializing)]
    pub end_slot: Slot,

    /// minimal balance for specific range
    pub min_balance: Amount,

    /// max rolls for specific range
    pub max_rolls: u64,
}

/// init the end_slot on startup
fn init_end_slot_range() -> Slot {
    Slot::new(0, 0)
}
