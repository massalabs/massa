//! Copyright (c) 2022 MASSA LABS <info@massa.net>

use massa_models::{ledger_models::LedgerData, Address, Amount, BlockId, Slot};
use massa_signature::{derive_public_key, generate_random_private_key};
use massa_time::MassaTime;
use serial_test::serial;
use std::str::FromStr;

use super::tools::*;
use massa_consensus_exports::ConsensusConfig;

/// # Context
///
/// Regression test for https://github.com/massalabs/massa/pull/2433
///
/// When there are 2 cliques C1 and C2, and a new block B arrives and makes C2 stale,
/// a whole batch of blocks from C1 will suddenly become final.
/// However, clique computation, and stale/final block identification is done
/// after checking the integrity of B and adding it to the graph.
/// And once all of that is done, PoS is informed of the newly finalized block batch.
/// Therefore, while B is being checked, PoS is not yet aware of the blocks
/// finalized by the addition of B to the graph.
///
/// On the other hand when verifying roll sales happening in B (before its addition to the graph),
/// we need to go back to the latest final block to which the final roll registry is attached,
/// and apply roll changes coming from subsequent active blocks
/// in order to get the current effective roll registry at the input of B.
/// The PoS module is the one holding the final roll registry.
///
/// This means that when getting the current final roll registry,
/// we must ask PoS for the final roll registry at the latest cycle with final blocks according to PoS,
/// and apply subsequent graph blocks, whether they are final or not according to block graph.
/// We must not assume that PoS has the same knowledge of final blocks as consensus.
///
/// # Test description
///
/// * extend 2 cliques C1 and C2
/// * push an incoming block B that contains valid a roll sale
/// * ensure that when B is added:
///   * C2 becomes stale
///   * a batch of blocks from B1 becomes final so that this batch spans across the limit of a cycle
///
/// If the lookup is badly implemented, B's verification step will fail
/// because Consensus and PoS are desynchronized in their final block/cycle knowledge.
/// If the the lookup is implemented correctly, B will be propagated.
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
        periods_per_cycle: 4,
        delta_f0: 2,
        thread_count: 1,
        endorsement_count: 2,
        roll_price,
        t0,
        future_block_processing_max_periods: 50,
        genesis_timestamp: MassaTime::now().unwrap().saturating_add(warmup_time),
        ..ConsensusConfig::default_with_staking_keys_and_ledger(&vec![staking_key], &initial_ledger)
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

            // Graph (1 thread):
            //            cycle limit
            //                v
            // G - M - A1 - M - A2 - A3
            //   \
            //    B1 - M  - B2
            //
            // Where:
            // G = genesis
            // M = miss
            // A1 = block buying 1 roll, 0 endorsements
            // A2 = empty block, 0 endorsements
            // A3 = block selling 1 roll, 2 endorsements
            // B1, B2 = empty blocks of the alternative clique, 0 endorsements
            //
            // When A3 arrives:
            // * B1 and B2 become stale
            // * A1, A2 become final
            // * A3 should be propagated

            // Create, send and propagate B1
            let (b1_id, b1_block, _) =
                create_block(&cfg, Slot::new(1, 0), genesis_blocks.clone(), staking_key);
            protocol_controller.receive_block(b1_block.clone()).await;
            validate_propagate_block(
                &mut protocol_controller,
                b1_id,
                t0.saturating_add(margin_time).to_millis(),
            )
            .await;

            // Create, send and propagate A1
            let roll_buy = create_roll_buy(staking_key, 1, 2, 0);
            let (a1_id, a1_block, _) = create_block_with_operations(
                &cfg,
                Slot::new(2, 0),
                &genesis_blocks,
                staking_key,
                vec![roll_buy],
            );
            protocol_controller.receive_block(a1_block.clone()).await;
            validate_propagate_block(
                &mut protocol_controller,
                a1_id,
                t0.saturating_add(margin_time).to_millis(),
            )
            .await;

            // Create, send and propagate B2
            let (b2_id, b2_block, _) =
                create_block(&cfg, Slot::new(3, 0), vec![b1_id], staking_key);
            protocol_controller.receive_block(b2_block.clone()).await;
            validate_propagate_block(
                &mut protocol_controller,
                b2_id,
                t0.saturating_add(margin_time).to_millis(),
            )
            .await;

            // Create, send and propagate A2
            let (a2_id, a2_block, _) =
                create_block(&cfg, Slot::new(4, 0), vec![a1_id], staking_key);
            protocol_controller.receive_block(a2_block.clone()).await;
            validate_propagate_block(
                &mut protocol_controller,
                a2_id,
                t0.saturating_add(margin_time).to_millis(),
            )
            .await;

            // Create, send and propagate A3
            let roll_sell = create_roll_sell(staking_key, 2, 5, 0);
            let endorsement1 = create_endorsement(staking_key, Slot::new(4, 0), a2_id, 0);
            let endorsement2 = create_endorsement(staking_key, Slot::new(4, 0), a2_id, 1);
            let (a3_id, a3_block, _) = create_block_with_operations_and_endorsements(
                &cfg,
                Slot::new(5, 0),
                &vec![a2_id],
                staking_key,
                vec![roll_sell],
                vec![endorsement1, endorsement2],
            );
            protocol_controller.receive_block(a3_block.clone()).await;
            validate_propagate_block(
                &mut protocol_controller,
                a3_id,
                t0.saturating_add(margin_time).to_millis(),
            )
            .await;

            (
                protocol_controller,
                consensus_command_sender,
                consensus_event_receiver,
            )
        },
    )
    .await;
}
