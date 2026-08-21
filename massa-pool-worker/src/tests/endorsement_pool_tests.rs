use std::{collections::BTreeMap, time::Duration};

use massa_models::{address::Address, config::THREAD_COUNT, prehash::PreHashSet, slot::Slot};
use massa_pool_exports::PoolConfig;
use massa_pos_exports::{MockSelectorController, Selection};
use massa_signature::KeyPair;

use super::tools::{
    create_endorsement, create_endorsement_on_block, default_mock_execution_controller, pool_test,
};
use crate::endorsement_pool::MAX_ENDORSEMENTS_PER_SLOT_INDEX;

fn create_selector_mock_with_address(address: Address) -> MockSelectorController {
    let mut res = MockSelectorController::new();
    res.expect_clone_box()
        .returning(move || Box::new(create_selector_mock_with_address(address)));
    res.expect_get_selection().returning(move |_| {
        Ok(Selection {
            producer: address,
            endorsements: vec![address; 16],
        })
    });
    res.expect_get_available_selections_in_range()
        .returning(move |slots, _| {
            let mut result = BTreeMap::default();
            let start = slots.start();
            let end = slots.end();
            let mut current = *start;
            while current <= *end {
                result.insert(
                    current,
                    Selection {
                        producer: address,
                        endorsements: vec![address; 16],
                    },
                );
                current = current.get_next_slot(THREAD_COUNT).unwrap();
            }
            Ok(result)
        });
    res
}

fn default_mock_selector(address: Address) -> Box<MockSelectorController> {
    Box::new(create_selector_mock_with_address(address))
}

fn create_selector_mock_bad_pos(address: Address) -> MockSelectorController {
    let mut res = MockSelectorController::new();
    res.expect_clone_box()
        .returning(move || Box::new(create_selector_mock_bad_pos(address)));
    res.expect_get_selection().returning(move |_| {
        // We make a new address and so the PoS draw isn't correct
        let sender_keypair = KeyPair::generate(0).unwrap();
        let address2 = Address::from_public_key(&sender_keypair.get_public_key());
        Ok(Selection {
            producer: address,
            endorsements: vec![address2; 16],
        })
    });
    res.expect_get_available_selections_in_range()
        .returning(move |slots, _| {
            let mut result = BTreeMap::default();
            let start = slots.start();
            let end = slots.end();
            let mut current = *start;
            while current <= *end {
                result.insert(
                    current,
                    Selection {
                        producer: address,
                        endorsements: vec![address; 16],
                    },
                );
                current = current.get_next_slot(THREAD_COUNT).unwrap();
            }
            Ok(result)
        });
    res
}

#[test]
fn test_add_endorsements() {
    let sender_keypair = KeyPair::generate(0).unwrap();
    let address = Address::from_public_key(&sender_keypair.get_public_key());
    let execution_controller = default_mock_execution_controller();
    let selector_controller = default_mock_selector(address);
    pool_test(
        PoolConfig::default(),
        execution_controller,
        selector_controller,
        Some((address, sender_keypair.clone())),
        |mut pool, mut storage| {
            let endorsements = vec![
                create_endorsement(&sender_keypair, 0, Slot::new(1, 2)),
                create_endorsement(&sender_keypair, 0, Slot::new(1, 3)),
            ];
            storage.store_endorsements(endorsements);
            pool.add_endorsements(storage.clone());
            // Allow some time for the pool to add the endorsements
            std::thread::sleep(Duration::from_secs(2));
            assert_eq!(
                pool.get_endorsement_count(None)
                    .expect("Failed to get endorsement count"),
                2
            );
        },
    );
}

#[test]
fn test_dont_add_endorsements_bad_pos() {
    let sender_keypair = KeyPair::generate(0).unwrap();
    let address = Address::from_public_key(&sender_keypair.get_public_key());
    let execution_controller = default_mock_execution_controller();
    let selector_controller = Box::new(create_selector_mock_bad_pos(address));

    pool_test(
        PoolConfig::default(),
        execution_controller,
        selector_controller,
        Some((address, sender_keypair.clone())),
        |mut pool, mut storage| {
            let endorsements = vec![
                create_endorsement(&sender_keypair, 0, Slot::new(1, 2)),
                create_endorsement(&sender_keypair, 0, Slot::new(1, 3)),
            ];
            storage.store_endorsements(endorsements);
            pool.add_endorsements(storage.clone());
            // Allow some time for the pool to add the endorsements
            std::thread::sleep(Duration::from_secs(2));
            assert_eq!(
                pool.get_endorsement_count(None)
                    .expect("Failed to get endorsement count"),
                0
            );
        },
    );
}

#[test]
fn test_dont_add_endorsements_outdated() {
    let sender_keypair = KeyPair::generate(0).unwrap();
    let address = Address::from_public_key(&sender_keypair.get_public_key());
    let execution_controller = default_mock_execution_controller();
    let selector_controller = default_mock_selector(address);

    pool_test(
        PoolConfig::default(),
        execution_controller,
        selector_controller,
        Some((address, sender_keypair.clone())),
        |mut pool, mut storage| {
            let endorsements = vec![
                create_endorsement(&sender_keypair, 0, Slot::new(1, 2)),
                create_endorsement(&sender_keypair, 0, Slot::new(1, 3)),
            ];
            storage.store_endorsements(endorsements);
            // Increase the final cs period so that our endorsements should be refused
            pool.notify_final_cs_periods(&vec![1; THREAD_COUNT as usize]);
            pool.add_endorsements(storage.clone());
            // Allow some time for the pool to add the endorsements
            std::thread::sleep(Duration::from_secs(2));
            assert_eq!(
                pool.get_endorsement_count(None)
                    .expect("Failed to get endorsement count"),
                0
            );
        },
    );
}

#[test]
fn test_dont_add_endorsements_pool_full() {
    let sender_keypair = KeyPair::generate(0).unwrap();
    let address = Address::from_public_key(&sender_keypair.get_public_key());
    let execution_controller = default_mock_execution_controller();
    let selector_controller = default_mock_selector(address);
    let cfg = PoolConfig {
        max_endorsements_pool_size_per_thread: 1,
        ..Default::default()
    };
    pool_test(
        cfg,
        execution_controller,
        selector_controller,
        Some((address, sender_keypair.clone())),
        |mut pool, mut storage| {
            let endorsements = vec![
                create_endorsement(&sender_keypair, 0, Slot::new(1, 2)),
                create_endorsement(&sender_keypair, 0, Slot::new(2, 2)),
            ];
            storage.store_endorsements(endorsements);
            pool.add_endorsements(storage.clone());
            // Allow some time for the pool to add the endorsements
            std::thread::sleep(Duration::from_secs(2));
            assert_eq!(
                pool.get_endorsement_count(None)
                    .expect("Failed to get endorsement count"),
                1
            );
        },
    );
}

#[test]
fn test_remove_endorsements_pool_outdated() {
    let sender_keypair = KeyPair::generate(0).unwrap();
    let address = Address::from_public_key(&sender_keypair.get_public_key());
    let execution_controller = default_mock_execution_controller();
    let selector_controller = default_mock_selector(address);
    pool_test(
        PoolConfig::default(),
        execution_controller,
        selector_controller,
        Some((address, sender_keypair.clone())),
        |mut pool, mut storage| {
            let endorsements = vec![
                create_endorsement(&sender_keypair, 0, Slot::new(1, 2)),
                create_endorsement(&sender_keypair, 0, Slot::new(2, 2)),
            ];
            storage.store_endorsements(endorsements.clone());
            pool.add_endorsements(storage.clone());
            // Allow some time for the pool to add the endorsements
            std::thread::sleep(Duration::from_secs(2));
            pool.notify_final_cs_periods(&vec![1; THREAD_COUNT as usize]);
            std::thread::sleep(Duration::from_secs(2));
            assert_eq!(
                pool.contains_endorsements(&[endorsements[0].id, endorsements[1].id], None)
                    .expect("Failed to check contains endorsements"),
                vec![false, true]
            );
            assert_eq!(
                pool.get_endorsement_count(None)
                    .expect("Failed to get endorsement count"),
                1
            );
        },
    );
}

#[test]
fn test_get_block_endorsements_works() {
    let sender_keypair = KeyPair::generate(0).unwrap();
    let address = Address::from_public_key(&sender_keypair.get_public_key());
    let execution_controller = default_mock_execution_controller();
    let selector_controller = default_mock_selector(address);

    pool_test(
        PoolConfig::default(),
        execution_controller,
        selector_controller,
        Some((address, sender_keypair.clone())),
        |mut pool, mut storage| {
            let endorsements = vec![
                create_endorsement(&sender_keypair, 0, Slot::new(1, 2)),
                create_endorsement(&sender_keypair, 1, Slot::new(1, 2)),
            ];
            storage.store_endorsements(endorsements.clone());
            pool.add_endorsements(storage.clone());
            // Allow some time for the pool to add the endorsements
            std::thread::sleep(Duration::from_secs(2));
            let (endorsement_ids, endorsements_storage) = pool
                .get_block_endorsements(
                    &endorsements[0].content.endorsed_block,
                    &Slot::new(1, 2),
                    None,
                )
                .expect("Failed to get block endorsements");
            assert_eq!(endorsement_ids.iter().filter(|id| id.is_some()).count(), 2);
            assert!(endorsement_ids[0].is_some());
            assert!(endorsement_ids[1].is_some());
            assert_eq!(endorsements_storage.get_endorsement_refs().len(), 2);
        },
    );
}

/// A drawn endorser can sign arbitrarily many valid endorsements for the same `(slot, index)`
/// that only differ by their endorsed block. The pool must only keep a bounded number of them.
#[test]
fn test_bound_conflicting_endorsements_per_slot_index() {
    let sender_keypair = KeyPair::generate(0).unwrap();
    let address = Address::from_public_key(&sender_keypair.get_public_key());
    let execution_controller = default_mock_execution_controller();
    let selector_controller = default_mock_selector(address);
    pool_test(
        PoolConfig::default(),
        execution_controller,
        selector_controller,
        Some((address, sender_keypair.clone())),
        |mut pool, mut storage| {
            let endorsements: Vec<_> = (0..10)
                .map(|i| {
                    create_endorsement_on_block(
                        &sender_keypair,
                        0,
                        Slot::new(1, 2),
                        &format!("conflicting_block_{}", i),
                    )
                })
                .collect();
            storage.store_endorsements(endorsements);
            pool.add_endorsements(storage.clone());
            // Allow some time for the pool to add the endorsements
            std::thread::sleep(Duration::from_secs(2));
            assert_eq!(
                pool.get_endorsement_count(None)
                    .expect("Failed to get endorsement count"),
                MAX_ENDORSEMENTS_PER_SLOT_INDEX
            );
        },
    );
}

/// The bound must leave room for more than one endorsed block per `(slot, index)`: an equivocating
/// endorser sending a variant that does not match the parent our block factory settles on must not
/// be able to deny us the endorsement variant that does match it.
#[test]
fn test_conflicting_endorsement_does_not_deny_the_matching_one() {
    let sender_keypair = KeyPair::generate(0).unwrap();
    let address = Address::from_public_key(&sender_keypair.get_public_key());
    let execution_controller = default_mock_execution_controller();
    let selector_controller = default_mock_selector(address);
    pool_test(
        PoolConfig::default(),
        execution_controller,
        selector_controller,
        Some((address, sender_keypair.clone())),
        |mut pool, mut storage| {
            // fill the draw with conflicting variants, one of which endorses the parent our block
            // factory will settle on. Insertion order is not controlled, so stay at the bound to
            // assert order-independently that the matching variant is never the one evicted.
            let matching =
                create_endorsement_on_block(&sender_keypair, 0, Slot::new(1, 2), "our_parent");
            let mut endorsements: Vec<_> = (0..MAX_ENDORSEMENTS_PER_SLOT_INDEX - 1)
                .map(|i| {
                    create_endorsement_on_block(
                        &sender_keypair,
                        0,
                        Slot::new(1, 2),
                        &format!("other_tip_{}", i),
                    )
                })
                .collect();
            endorsements.push(matching.clone());
            let target_block = matching.content.endorsed_block;
            storage.store_endorsements(endorsements);
            pool.add_endorsements(storage.clone());
            // Allow some time for the pool to add the endorsements
            std::thread::sleep(Duration::from_secs(2));

            let (endorsement_ids, _) = pool
                .get_block_endorsements(&target_block, &Slot::new(1, 2), None)
                .expect("Failed to get block endorsements");
            assert_eq!(endorsement_ids[0], Some(matching.id));
        },
    );
}

/// Pruning used to drop an endorsement from the sort index only, leaving the lookup index pointing
/// at an endorsement whose storage reference had been released. Looking that endorsement up for
/// block creation then panicked, halting the pool worker.
#[test]
fn test_pruned_endorsements_are_not_returned_for_block_creation() {
    let sender_keypair = KeyPair::generate(0).unwrap();
    let address = Address::from_public_key(&sender_keypair.get_public_key());
    let execution_controller = default_mock_execution_controller();
    let selector_controller = default_mock_selector(address);
    let cfg = PoolConfig {
        max_endorsements_pool_size_per_thread: 1,
        ..Default::default()
    };
    pool_test(
        cfg,
        execution_controller,
        selector_controller,
        Some((address, sender_keypair.clone())),
        |mut pool, mut storage| {
            // both endorsements land in thread 2, so the second one is pruned away by the size cap
            let kept = create_endorsement(&sender_keypair, 0, Slot::new(1, 2));
            let pruned = create_endorsement(&sender_keypair, 0, Slot::new(2, 2));
            storage.store_endorsements(vec![kept.clone(), pruned.clone()]);
            pool.add_endorsements(storage.clone());
            // Allow some time for the pool to add the endorsements
            std::thread::sleep(Duration::from_secs(2));
            assert_eq!(
                pool.contains_endorsements(&[kept.id, pruned.id], None)
                    .expect("Failed to check contains endorsements"),
                vec![true, false]
            );

            // release the caller-side reference so that nothing keeps the pruned endorsement alive
            // in storage anymore
            storage.drop_endorsement_refs(&PreHashSet::from_iter([pruned.id]));

            // the pruned endorsement must not be handed out for block creation (used to panic)
            let (endorsement_ids, endorsements_storage) = pool
                .get_block_endorsements(&pruned.content.endorsed_block, &Slot::new(2, 2), None)
                .expect("Failed to get block endorsements");
            assert!(endorsement_ids.iter().all(|id| id.is_none()));
            assert!(endorsements_storage.get_endorsement_refs().is_empty());

            // the endorsement that was kept is still available
            let (endorsement_ids, _) = pool
                .get_block_endorsements(&kept.content.endorsed_block, &Slot::new(1, 2), None)
                .expect("Failed to get block endorsements");
            assert_eq!(endorsement_ids[0], Some(kept.id));
        },
    );
}

/// After finalization pruning, nothing must be left behind in the pool for the finalized slots.
#[test]
fn test_finalization_leaves_no_orphan_entry() {
    let sender_keypair = KeyPair::generate(0).unwrap();
    let address = Address::from_public_key(&sender_keypair.get_public_key());
    let execution_controller = default_mock_execution_controller();
    let selector_controller = default_mock_selector(address);
    pool_test(
        PoolConfig::default(),
        execution_controller,
        selector_controller,
        Some((address, sender_keypair.clone())),
        |mut pool, mut storage| {
            let endorsement = create_endorsement(&sender_keypair, 0, Slot::new(1, 2));
            storage.store_endorsements(vec![endorsement.clone()]);
            pool.add_endorsements(storage.clone());
            std::thread::sleep(Duration::from_secs(2));
            pool.notify_final_cs_periods(&vec![1; THREAD_COUNT as usize]);
            std::thread::sleep(Duration::from_secs(2));

            assert_eq!(
                pool.get_endorsement_count(None)
                    .expect("Failed to get endorsement count"),
                0
            );
            let (endorsement_ids, _) = pool
                .get_block_endorsements(&endorsement.content.endorsed_block, &Slot::new(1, 2), None)
                .expect("Failed to get block endorsements");
            assert!(endorsement_ids.iter().all(|id| id.is_none()));
        },
    );
}
