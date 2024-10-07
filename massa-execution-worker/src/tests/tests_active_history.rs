use crate::active_history::ActiveHistory;
use massa_execution_exports::ExecutionOutput;
use massa_models::slot::Slot;
use std::collections::{BTreeMap, VecDeque};

use massa_final_state::StateChanges;
use massa_hash::Hash;
use massa_models::address::{Address, UserAddress, UserAddressV0};
use massa_models::amount::Amount;
use massa_models::prehash::{CapacityAllocator, PreHashMap};
use massa_pos_exports::{DeferredCredits, PoSChanges};

#[test]
fn test_active_history_deferred_credits() {
    let slot1 = Slot::new(2, 2);
    let slot2 = Slot::new(4, 11);

    let addr1 = Address::User(UserAddress::UserAddressV0(UserAddressV0(
        Hash::compute_from("AU1".as_bytes()),
    )));
    let addr2 = Address::User(UserAddress::UserAddressV0(UserAddressV0(
        Hash::compute_from("AU2".as_bytes()),
    )));

    let amount_a1_s1 = Amount::from_raw(500);
    let amount_a2_s1 = Amount::from_raw(2702);
    let amount_a1_s2 = Amount::from_raw(37);
    let amount_a2_s2 = Amount::from_raw(3);

    let mut ph1 = PreHashMap::with_capacity(2);
    ph1.insert(addr1, amount_a1_s1);
    ph1.insert(addr2, amount_a2_s1);
    let mut ph2 = PreHashMap::with_capacity(2);
    ph2.insert(addr1, amount_a1_s2);
    ph2.insert(addr2, amount_a2_s2);

    let mut credits = DeferredCredits::new();
    credits.credits = BTreeMap::from([(slot1, ph1), (slot2, ph2)]);

    let exec_output_1 = ExecutionOutput {
        slot: Slot::new(1, 0),
        block_info: None,
        state_changes: StateChanges {
            ledger_changes: Default::default(),
            async_pool_changes: Default::default(),
            deferred_call_changes: Default::default(),
            pos_changes: PoSChanges {
                seed_bits: Default::default(),
                roll_changes: Default::default(),
                production_stats: Default::default(),
                deferred_credits: credits,
            },
            executed_ops_changes: Default::default(),
            executed_denunciations_changes: Default::default(),
            execution_trail_hash_change: Default::default(),
        },
        events: Default::default(),
        #[cfg(feature = "execution-trace")]
        slot_trace: Default::default(),
        #[cfg(feature = "dump-block")]
        storage: None,
        deferred_credits_execution: Default::default(),
        cancel_async_message_execution: Default::default(),
        auto_sell_execution: Default::default(),
    };

    let active_history = ActiveHistory(VecDeque::from([exec_output_1]));

    assert_eq!(
        active_history.get_adress_deferred_credit_for(&addr1, &slot2),
        Some(amount_a1_s2)
    );

    let deferred_credit_for_slot1 = active_history.get_all_deferred_credits_until(&slot1);
    assert_eq!(
        deferred_credit_for_slot1.get_address_credits_for_slot(&addr1, &slot1),
        Some(amount_a1_s1)
    );
    assert_eq!(
        deferred_credit_for_slot1.get_address_credits_for_slot(&addr2, &slot1),
        Some(amount_a2_s1)
    );
}
