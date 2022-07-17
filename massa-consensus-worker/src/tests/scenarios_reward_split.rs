// Copyright (c) 2022 MASSA LABS <info@massa.net>

use super::tools::*;
use massa_consensus_exports::ConsensusConfig;
use massa_models::ledger_models::LedgerData;
use massa_models::wrapped::WrappedContent;
use massa_models::{Address, Amount, BlockId, Endorsement, EndorsementSerializer, Slot};
use massa_time::MassaTime;
use serial_test::serial;
use std::collections::HashMap;
use std::str::FromStr;

#[tokio::test]
#[serial]
async fn test_reward_split() {
    // setup logging
    // stderrlog::new()
    // .verbosity(2)
    // .timestamp(stderrlog::Timestamp::Millisecond)
    // .init()
    // .unwrap();
    let thread_count = 2;

    // Create addresses
    let (address_a, keypair_a) = random_address_on_thread(0, thread_count).into();
    let (address_b, keypair_b) = random_address_on_thread(0, thread_count).into();

    let mut ledger = HashMap::new();
    ledger.insert(address_a, LedgerData::new(Amount::from_str("10").unwrap()));
    ledger.insert(address_b, LedgerData::new(Amount::from_str("10").unwrap()));
    let staking_keys = vec![keypair_a.clone(), keypair_b.clone()];
    let init_time: MassaTime = 1000.into();
    let cfg = ConsensusConfig {
        endorsement_count: 5,
        genesis_timestamp: MassaTime::now().unwrap().saturating_add(init_time),
        max_block_size: 2000,
        max_operations_per_block: 5000,
        operation_batch_size: 500,
        operation_validity_periods: 10,
        periods_per_cycle: 3,
        t0: 500.into(),
        ..ConsensusConfig::default_with_staking_keys_and_ledger(&staking_keys, &ledger)
    };

    consensus_without_pool_test(
        cfg.clone(),
        async move |mut protocol_controller, consensus_command_sender, consensus_event_receiver| {
            // Check initial balances.
            let addresses_state = consensus_command_sender
                .get_addresses_info(vec![address_a, address_b].into_iter().collect())
                .await
                .unwrap();

            let addresse_a_state = addresses_state.get(&address_a).unwrap();
            assert_eq!(
                addresse_a_state.ledger_info.candidate_ledger_info.balance,
                Amount::from_str("10").unwrap()
            );

            let addresse_b_state = addresses_state.get(&address_b).unwrap();
            assert_eq!(
                addresse_b_state.ledger_info.candidate_ledger_info.balance,
                Amount::from_str("10").unwrap()
            );

            let draws: HashMap<_, _> = consensus_command_sender
                .get_selection_draws(Slot::new(1, 0), Slot::new(3, 0))
                .await
                .unwrap()
                .into_iter()
                .collect();

            let slot_one_block_addr = draws.get(&Slot::new(1, 0)).unwrap().0;
            let slot_one_endorsements_addrs = draws.get(&Slot::new(1, 0)).unwrap().1.clone();

            let slot_one_keypair = if slot_one_block_addr == address_a {
                keypair_a.clone()
            } else {
                keypair_b.clone()
            };

            // Create, and propagate, block 1.
            let parents: Vec<BlockId> = consensus_command_sender
                .get_block_graph_status(None, None)
                .await
                .unwrap()
                .best_parents
                .iter()
                .map(|(b, _p)| *b)
                .collect();

            let b1 = create_block(&cfg, Slot::new(1, 0), parents, &slot_one_keypair);

            propagate_block(
                &mut protocol_controller,
                b1.clone(),
                true,
                init_time
                    .saturating_add(cfg.t0.saturating_mul(2))
                    .to_millis(),
            )
            .await;

            let slot_two_block_addr = draws.get(&Slot::new(2, 0)).unwrap().0;

            let slot_two_keypair = if slot_two_block_addr == address_a {
                keypair_a.clone()
            } else {
                keypair_b.clone()
            };

            // Create, and propagate, block 2.
            let parents: Vec<BlockId> = consensus_command_sender
                .get_block_graph_status(None, None)
                .await
                .unwrap()
                .best_parents
                .iter()
                .map(|(b, _p)| *b)
                .collect();
            assert!(parents.contains(&b1.id));

            let mut b2 = create_block(&cfg, Slot::new(2, 0), parents, &slot_two_keypair);

            // Endorsements in block 2.

            // Creator of second block endorses the first.
            let index = slot_one_endorsements_addrs
                .iter()
                .position(|&addr| {
                    addr == Address::from_public_key(&slot_two_keypair.get_public_key())
                })
                .unwrap() as u32;
            let content = Endorsement {
                slot: Slot::new(1, 0),
                index,
                endorsed_block: b1.id,
            };
            let ed_1 = Endorsement::new_wrapped(
                content.clone(),
                EndorsementSerializer::new(),
                &slot_two_keypair,
            )
            .unwrap();

            // Creator of first block endorses the first.
            let index = slot_one_endorsements_addrs
                .iter()
                .position(|&addr| {
                    addr == Address::from_public_key(&slot_one_keypair.get_public_key())
                })
                .unwrap() as u32;
            let content = Endorsement {
                slot: Slot::new(1, 0),
                index,
                endorsed_block: b1.id,
            };
            let ed_2 = Endorsement::new_wrapped(
                content.clone(),
                EndorsementSerializer::new(),
                &slot_one_keypair,
            )
            .unwrap();

            // Creator of second block endorses the first, again.
            let index = slot_one_endorsements_addrs
                .iter()
                .position(|&addr| {
                    addr == Address::from_public_key(&slot_two_keypair.get_public_key())
                })
                .unwrap() as u32;
            let content = Endorsement {
                slot: Slot::new(1, 0),
                index,
                endorsed_block: b1.id,
            };
            let ed_3 = Endorsement::new_wrapped(
                content.clone(),
                EndorsementSerializer::new(),
                &slot_two_keypair,
            )
            .unwrap();

            // Add endorsements to block.
            b2.content.header.content.endorsements = vec![ed_1, ed_2, ed_3];

            // Propagate block.
            tokio::time::sleep(cfg.t0.to_duration()).await;
            propagate_block(&mut protocol_controller, b2, true, 300).await;

            // Check balances after second block.
            let addresses_state = consensus_command_sender
                .get_addresses_info(vec![address_a, address_b].into_iter().collect())
                .await
                .unwrap();

            let third = cfg
                .block_reward
                .checked_div_u64((3 * (1 + cfg.endorsement_count)).into())
                .unwrap();

            let expected_a = Amount::from_str("10")
                .unwrap() // initial ledger
                .saturating_add(if keypair_a.to_bytes() == slot_one_keypair.to_bytes() {
                    // block 1 reward
                    cfg.block_reward
                        .checked_mul_u64(1)
                        .unwrap()
                        .checked_div_u64((1 + cfg.endorsement_count).into())
                        .unwrap()
                        .saturating_sub(third.checked_mul_u64(0).unwrap())
                        // endorsements reward
                        .saturating_add(
                            third // parent in ed 1
                                .saturating_add(third) // creator of ed 2
                                .saturating_add(third) // parent in ed 2
                                .saturating_add(third), // parent in ed 3
                        )
                } else {
                    Default::default()
                })
                .saturating_add(if keypair_a.to_bytes() == slot_two_keypair.to_bytes() {
                    // block 2 creation reward
                    cfg.block_reward
                        .checked_mul_u64(1 + 3)
                        .unwrap()
                        .checked_div_u64((1 + cfg.endorsement_count).into())
                        .unwrap()
                        .saturating_sub(third.checked_mul_u64(2 * 3).unwrap())
                        // endorsement rewards
                        .saturating_add(
                            third // creator of ed 1
                                .saturating_add(third), // creator of ed 3
                        )
                } else {
                    Default::default()
                });

            let expected_b = Amount::from_str("10")
                .unwrap() // initial ledger
                .saturating_add(if keypair_b.to_bytes() == slot_one_keypair.to_bytes() {
                    // block 1 reward
                    cfg.block_reward
                        .checked_mul_u64(1)
                        .unwrap()
                        .checked_div_u64((1 + cfg.endorsement_count).into())
                        .unwrap()
                        .saturating_sub(third.checked_mul_u64(0).unwrap())
                        // endorsements reward
                        .saturating_add(
                            third // parent in ed 1
                                .saturating_add(third) // creator of ed 2
                                .saturating_add(third) // parent in ed 2
                                .saturating_add(third), // parent in ed 3
                        )
                } else {
                    Default::default()
                })
                .saturating_add(if keypair_b.to_bytes() == slot_two_keypair.to_bytes() {
                    // block 2 creation reward
                    cfg.block_reward
                        .checked_mul_u64(1 + 3)
                        .unwrap()
                        .checked_div_u64((1 + cfg.endorsement_count).into())
                        .unwrap()
                        .saturating_sub(third.checked_mul_u64(2 * 3).unwrap())
                        // endorsement rewards
                        .saturating_add(
                            third // creator of ed 1
                                .saturating_add(third), // creator of ed 3
                        )
                } else {
                    Default::default()
                });

            let state_a = addresses_state.get(&address_a).unwrap();
            assert_eq!(
                state_a.ledger_info.candidate_ledger_info.balance,
                expected_a
            );

            let state_b = addresses_state.get(&address_b).unwrap();
            assert_eq!(
                state_b.ledger_info.candidate_ledger_info.balance,
                expected_b
            );

            (
                protocol_controller,
                consensus_command_sender,
                consensus_event_receiver,
            )
        },
    )
    .await;
}
