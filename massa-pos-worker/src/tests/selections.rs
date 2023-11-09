use massa_hash::Hash;
use massa_models::address::Address;
use massa_models::slot::Slot;
use massa_pos_exports::SelectorConfig;
use rand::thread_rng;
use rand::RngCore;
use std::{collections::BTreeMap, str::FromStr};

use crate::start_selector_worker;

#[test]
fn basic() {
    // initialize the selector configuration and the test inputs
    let cfg = SelectorConfig::default();
    let mut lookback_rolls: BTreeMap<Address, u64> = std::collections::BTreeMap::new();
    lookback_rolls.insert(
        Address::from_str("AU12Cyu2f7C7isA3ADAhoNuq9ZUFPKP24jmiGj3sh9D1pHoAWKDYY").unwrap(),
        1,
    );
    lookback_rolls.insert(
        Address::from_str("AU12BTfZ7k1z6PsLEUZeHYNirz6WJ3NdrWto9H4TkVpkV9xE2TJg2").unwrap(),
        1,
    );
    let mut seed_bytes = [0u8; 16];
    thread_rng().fill_bytes(&mut seed_bytes);
    let lookback_seed = Hash::compute_from(&seed_bytes);

    // start the selector thread, get the controller and manager
    let (mut manager, controller) = start_selector_worker(cfg).unwrap();

    // feed the information used to compute the draws of a new cycle
    // this is supposed to take the rolls from C-3 and the seed from C-2
    // here we compute cycle 0 with dummy rolls and a random seed
    controller
        .feed_cycle(0, lookback_rolls, lookback_seed)
        .unwrap();

    // wait for the draws to compute
    controller.wait_for_draws(0).unwrap();

    // get the draws of 2 slots
    let two_slot_selection = controller
        .get_available_selections_in_range(
            Slot {
                period: 0,
                thread: 0,
            }..=Slot {
                period: 2,
                thread: 0,
            },
            None,
        )
        .unwrap();

    // 2 slots as inclusive range so 32 * 2 + 1 = 65
    // we expect 65 selections
    assert_eq!(two_slot_selection.len(), 65);

    // stop worker
    manager.stop();
}
