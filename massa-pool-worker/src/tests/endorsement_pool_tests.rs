use std::{collections::BTreeMap, time::Duration};

use massa_models::{address::Address, config::THREAD_COUNT, slot::Slot};
use massa_pool_exports::PoolConfig;
use massa_pos_exports::{MockSelectorController, Selection};
use massa_signature::KeyPair;

use super::tools::{create_endorsement, default_mock_execution_controller, pool_test};

fn default_mock_selector(address: Address) -> Box<MockSelectorController> {
    let mut res = Box::new(MockSelectorController::new());
    res.expect_clone_box().returning(move || {
        let mut res = Box::new(MockSelectorController::new());
        res.expect_get_selection().returning(move |_| {
            Ok(Selection {
                producer: address,
                endorsements: vec![address; 16],
            })
        });
        res.expect_get_available_selections_in_range()
            .returning(move |slots, _| {
                let mut res = BTreeMap::default();
                let start = slots.start();
                let end = slots.end();
                let mut current = *start;
                while current <= *end {
                    res.insert(
                        current,
                        Selection {
                            producer: address,
                            endorsements: vec![address; 16],
                        },
                    );
                    current = current.get_next_slot(THREAD_COUNT).unwrap();
                }
                Ok(res)
            });
        res
    });
    res.expect_get_selection().returning(move |_| {
        Ok(Selection {
            producer: address,
            endorsements: vec![address; 16],
        })
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
            let mut endorsements = Vec::new();
            endorsements.push(create_endorsement(&sender_keypair, 0, Slot::new(1, 2)));
            endorsements.push(create_endorsement(&sender_keypair, 0, Slot::new(1, 3)));
            storage.store_endorsements(endorsements);
            pool.add_endorsements(storage.clone());
            // Allow some time for the pool to add the endorsements
            std::thread::sleep(Duration::from_secs(2));
            assert_eq!(pool.get_endorsement_count(), 2);
        },
    );
}

#[test]
fn test_dont_add_endorsements_bad_pos() {
    let sender_keypair = KeyPair::generate(0).unwrap();
    let address = Address::from_public_key(&sender_keypair.get_public_key());
    let execution_controller = default_mock_execution_controller();
    let selector_controller = {
        let mut res = Box::new(MockSelectorController::new());
        res.expect_clone_box().returning(move || {
            let mut res = Box::new(MockSelectorController::new());
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
                    let mut res = BTreeMap::default();
                    let start = slots.start();
                    let end = slots.end();
                    let mut current = *start;
                    while current <= *end {
                        res.insert(
                            current,
                            Selection {
                                producer: address,
                                endorsements: vec![address; 16],
                            },
                        );
                        current = current.get_next_slot(THREAD_COUNT).unwrap();
                    }
                    Ok(res)
                });
            res
        });
        res.expect_get_selection().returning(move |_| {
            Ok(Selection {
                producer: address,
                endorsements: vec![address; 16],
            })
        });
        res
    };

    pool_test(
        PoolConfig::default(),
        execution_controller,
        selector_controller,
        Some((address, sender_keypair.clone())),
        |mut pool, mut storage| {
            let mut endorsements = Vec::new();
            endorsements.push(create_endorsement(&sender_keypair, 0, Slot::new(1, 2)));
            endorsements.push(create_endorsement(&sender_keypair, 0, Slot::new(1, 3)));
            storage.store_endorsements(endorsements);
            pool.add_endorsements(storage.clone());
            // Allow some time for the pool to add the endorsements
            std::thread::sleep(Duration::from_secs(2));
            assert_eq!(pool.get_endorsement_count(), 0);
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
            let mut endorsements = Vec::new();
            endorsements.push(create_endorsement(&sender_keypair, 0, Slot::new(1, 2)));
            endorsements.push(create_endorsement(&sender_keypair, 0, Slot::new(1, 3)));
            storage.store_endorsements(endorsements);
            // Increase the final cs period so that our endorsements should be refused
            pool.notify_final_cs_periods(&vec![1; THREAD_COUNT as usize]);
            pool.add_endorsements(storage.clone());
            // Allow some time for the pool to add the endorsements
            std::thread::sleep(Duration::from_secs(2));
            assert_eq!(pool.get_endorsement_count(), 0);
        },
    );
}

#[test]
fn test_dont_add_endorsements_pool_full() {
    let sender_keypair = KeyPair::generate(0).unwrap();
    let address = Address::from_public_key(&sender_keypair.get_public_key());
    let execution_controller = default_mock_execution_controller();
    let selector_controller = default_mock_selector(address);
    let mut cfg = PoolConfig::default();
    cfg.max_endorsements_pool_size_per_thread = 1;
    pool_test(
        cfg,
        execution_controller,
        selector_controller,
        Some((address, sender_keypair.clone())),
        |mut pool, mut storage| {
            let mut endorsements = Vec::new();
            endorsements.push(create_endorsement(&sender_keypair, 0, Slot::new(1, 2)));
            endorsements.push(create_endorsement(&sender_keypair, 0, Slot::new(2, 2)));
            storage.store_endorsements(endorsements);
            pool.add_endorsements(storage.clone());
            // Allow some time for the pool to add the endorsements
            std::thread::sleep(Duration::from_secs(2));
            assert_eq!(pool.get_endorsement_count(), 1);
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
            let mut endorsements = Vec::new();
            endorsements.push(create_endorsement(&sender_keypair, 0, Slot::new(1, 2)));
            endorsements.push(create_endorsement(&sender_keypair, 0, Slot::new(2, 2)));
            storage.store_endorsements(endorsements.clone());
            pool.add_endorsements(storage.clone());
            // Allow some time for the pool to add the endorsements
            std::thread::sleep(Duration::from_secs(2));
            pool.notify_final_cs_periods(&vec![1; THREAD_COUNT as usize]);
            std::thread::sleep(Duration::from_secs(2));
            assert_eq!(
                pool.contains_endorsements(&[endorsements[0].id, endorsements[1].id]),
                vec![false, true]
            );
            assert_eq!(pool.get_endorsement_count(), 1);
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
            let mut endorsements = Vec::new();
            endorsements.push(create_endorsement(&sender_keypair, 0, Slot::new(1, 2)));
            endorsements.push(create_endorsement(&sender_keypair, 1, Slot::new(1, 2)));
            storage.store_endorsements(endorsements.clone());
            pool.add_endorsements(storage.clone());
            // Allow some time for the pool to add the endorsements
            std::thread::sleep(Duration::from_secs(2));
            let (endorsement_ids, endorsements_storage) = pool
                .get_block_endorsements(&endorsements[0].content.endorsed_block, &Slot::new(1, 2));
            assert_eq!(endorsement_ids.iter().filter(|id| id.is_some()).count(), 2);
            assert!(endorsement_ids[0].is_some());
            assert!(endorsement_ids[1].is_some());
            assert_eq!(endorsements_storage.get_endorsement_refs().len(), 2);
        },
    );
}
