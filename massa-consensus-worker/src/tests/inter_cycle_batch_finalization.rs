//! Copyright (c) 2022 MASSA LABS <info@massa.net>

use super::tools::*;
use massa_consensus_exports::ConsensusConfig;
use massa_models::{ledger_models::LedgerData, Address, Amount, BlockId, Slot};
use massa_signature::{derive_public_key, generate_random_private_key};
use massa_time::MassaTime;
use serial_test::serial;
use std::{collections::HashSet, str::FromStr};

/// # Context
///
/// Regression test for `https://github.com/massalabs/massa/pull/2433`
///
/// When we have the following block sequence
/// ```
/// 1 thread, periods_per_cycle = 2, delta_f0 = 1, 1 endorsement per block
///
/// cycle 0 | cycle 1 | cycle 2
///  G - B1 - B2 - B3 - B4
/// where G is the genesis block
/// and B4 contains a roll sell operation
/// ```
///
/// And the block `B1` is received AFTER `B4`, blocks will be processed recursively:
/// ```
/// * B1 is received and included
/// * B2 is processed
/// * B1 becomes final in the graph
/// * B3 is processed
/// * B2 becomes final in the graph
/// * B4 is processed
/// * B3 becomes final in the graph
/// * PoS is told about all finalized blocks
/// ```
///
/// The problem we had is that in order to check rolls to verify `B4`'s roll sell,
/// the final roll registry was assumed to be attached to the last final block known by the graph,
/// but that was inaccurate because PoS was the one holding the final roll registry,
/// and PoS was not yet aware of the blocks that finalized during recursion,
/// so it was actually still attached to G when `B4` was checked.
///
/// The correction involved taking the point of view of PoS on where the final roll registry is attached.
/// This test ensures non-regression by making sure `B4` is propagated when `B1` is received.
#[tokio::test]
#[serial]
async fn test_inter_cycle_batch_finalization() {
    let t0: MassaTime = 1000.into();
    let staking_key = generate_random_private_key();
    let creator_public_key = derive_public_key(&staking_key);
    let creator_addr = Address::from_public_key(&creator_public_key);
    let roll_price = Amount::from_str("42").unwrap();
    let initial_ledger = vec![(
        creator_addr,
        LedgerData {
            balance: roll_price, // allows the address to buy 1 roll
        },
    )]
    .into_iter()
    .collect();
    let warmup_time: MassaTime = 1000.into();
    let margin_time: MassaTime = 300.into();
    let cfg = ConsensusConfig {
        periods_per_cycle: 2,
        delta_f0: 1,
        thread_count: 1,
        endorsement_count: 1,
        max_future_processing_blocks: 10,
        max_dependency_blocks: 10,
        future_block_processing_max_periods: 10,
        roll_price,
        t0,
        genesis_timestamp: MassaTime::now().unwrap().saturating_add(warmup_time),
        ..ConsensusConfig::default_with_staking_keys_and_ledger(&[staking_key], &initial_ledger)
    };

    consensus_without_pool_test(
        cfg.clone(),
        async move |mut protocol_controller, consensus_command_sender, consensus_event_receiver| {
            // wait for consensus warmup time
            tokio::time::sleep(warmup_time.to_duration()).await;

            let genesis_blocks: Vec<BlockId> = consensus_command_sender
                .get_block_graph_status(None, None)
                .await
                .expect("could not get block graph status")
                .best_parents
                .iter()
                .map(|(b, _p)| *b)
                .collect();

            // create B1 but DO NOT SEND IT
            tokio::time::sleep(t0.to_duration()).await;
            let (b1_id, b1_block, _) =
                create_block(&cfg, Slot::new(1, 0), genesis_blocks.clone(), staking_key);

            // create and send B2
            tokio::time::sleep(t0.to_duration()).await;
            let (b2_id, b2_block, _) = create_block_with_operations_and_endorsements(
                &cfg,
                Slot::new(2, 0),
                &vec![b1_id],
                staking_key,
                vec![],
                vec![create_endorsement(staking_key, Slot::new(1, 0), b1_id, 0)],
            );
            protocol_controller.receive_block(b2_block.clone()).await;

            // create and send B3
            tokio::time::sleep(t0.to_duration()).await;
            let (b3_id, b3_block, _) = create_block_with_operations_and_endorsements(
                &cfg,
                Slot::new(3, 0),
                &vec![b2_id],
                staking_key,
                vec![],
                vec![create_endorsement(staking_key, Slot::new(2, 0), b2_id, 0)],
            );
            protocol_controller.receive_block(b3_block.clone()).await;

            // create and send B4
            tokio::time::sleep(t0.to_duration()).await;
            let roll_sell = create_roll_sell(staking_key, 1, 4, 0);
            let (b4_id, b4_block, _) = create_block_with_operations_and_endorsements(
                &cfg,
                Slot::new(4, 0),
                &vec![b3_id],
                staking_key,
                vec![roll_sell],
                vec![create_endorsement(staking_key, Slot::new(3, 0), b3_id, 0)],
            );
            protocol_controller.receive_block(b4_block.clone()).await;

            // wait for the slot after B4
            tokio::time::sleep(t0.saturating_mul(5).to_duration()).await;

            // send B1
            protocol_controller.receive_block(b1_block.clone()).await;

            // wait for the propagation of B1, B2, B3 and B4 (unordered)
            let mut to_propagate: HashSet<_> =
                vec![b1_id, b2_id, b3_id, b4_id].into_iter().collect();
            for _ in 0u8..4 {
                to_propagate.remove(
                    &validate_propagate_block_in_list(
                        &mut protocol_controller,
                        &to_propagate.clone().into_iter().collect(),
                        margin_time.to_millis(),
                    )
                    .await,
                );
            }

            (
                protocol_controller,
                consensus_command_sender,
                consensus_event_receiver,
            )
        },
    )
    .await;
}
