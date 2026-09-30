//! Copyright (c) 2022 MASSA LABS <info@massa.net>

mod block_factory;
mod endorsement_factory;
mod manager;
mod run;

pub use run::start_factory;

use massa_models::{
    slot::Slot,
    timeslots::{get_block_slot_timestamp, get_latest_block_slot_at_timestamp},
};
use massa_time::MassaTime;

/// Returns the first slot whose timestamp is at or after `timestamp`.
fn get_first_slot_at_or_after_timestamp(
    thread_count: u8,
    t0: MassaTime,
    genesis_timestamp: MassaTime,
    timestamp: MassaTime,
) -> Slot {
    let Some(latest_slot) =
        get_latest_block_slot_at_timestamp(thread_count, t0, genesis_timestamp, timestamp)
            .expect("could not get latest block slot at timestamp")
    else {
        // we are before genesis
        return Slot::new(0, 0);
    };
    let latest_slot_timestamp =
        get_block_slot_timestamp(thread_count, t0, genesis_timestamp, latest_slot)
            .expect("could not get block slot timestamp");
    if latest_slot_timestamp == timestamp {
        latest_slot
    } else {
        latest_slot
            .get_next_slot(thread_count)
            .expect("could not compute next slot")
    }
}

#[cfg(test)]
mod tests;
