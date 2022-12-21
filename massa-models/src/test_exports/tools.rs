use massa_time::MassaTime;

use crate::timeslots::{get_block_slot_timestamp, get_closest_slot_to_timestamp};

/// Gets the instant of the next slot.
pub fn get_next_slot_instant(
    genesis_timestamp: MassaTime,
    thread_count: u8,
    t0: MassaTime,
) -> MassaTime {
    // get current time
    let now = MassaTime::now().expect("could not get current time");

    // get closest slot according to the current absolute time
    let mut slot = get_closest_slot_to_timestamp(thread_count, t0, genesis_timestamp, now);

    // ignore genesis
    if slot.period == 0 {
        slot.period = 1;
    }

    let actual_slot_time =
        get_block_slot_timestamp(thread_count, t0, genesis_timestamp, slot).unwrap();

    if actual_slot_time < now {
        let next_slot = slot.get_next_slot(thread_count).unwrap();
        get_block_slot_timestamp(thread_count, t0, genesis_timestamp, next_slot)
            .expect("could not get block slot timestamp")
    } else {
        actual_slot_time
    }

    // get the timestamp of the target slot
}
