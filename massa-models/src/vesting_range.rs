use crate::amount::Amount;
use crate::slot::Slot;
use massa_time::MassaTime;
use serde::{Deserialize, Serialize};

/// Represent a vesting range
#[derive(Clone, Copy, Deserialize, Serialize, Debug)]
pub struct VestingRange {
    /// start slot of range
    #[serde(default = "Slot::min")]
    #[serde(skip_serializing)]
    pub start_slot: Slot,

    /// end slot for the range
    /// Init with 0,0 and calculate on load
    #[serde(default = "Slot::min")]
    #[serde(skip_serializing)]
    pub end_slot: Slot,

    /// timestamp to get the start slot
    pub timestamp: MassaTime,

    /// minimal balance for specific range
    pub min_balance: Amount,

    /// max rolls for specific range
    pub max_rolls: u64,
}
