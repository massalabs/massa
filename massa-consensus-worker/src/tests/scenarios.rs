use std::collections::{HashSet, VecDeque};

use crate::tests::tools::create_block;
use massa_consensus_exports::ConsensusConfig;
use massa_models::{address::Address, block_id::BlockId, slot::Slot};
use massa_signature::KeyPair;
use massa_storage::Storage;
use massa_time::MassaTime;

use super::tools::{
    answer_ask_producer_pos, answer_ask_selection_pos, consensus_without_pool_test, register_block,
    validate_propagate_block_in_list,
};

#[tokio::test]
async fn test_unsorted_block() {
    let staking_key: KeyPair = KeyPair::generate();
    let cfg = ConsensusConfig {
        t0: 1000.into(),
        thread_count: 2,
        genesis_timestamp: MassaTime::now().unwrap(),
        future_block_processing_max_periods: 50,
        max_future_processing_blocks: 10,
        genesis_key: staking_key.clone(),
        ..ConsensusConfig::default()
    };

    let storage = Storage::create_root();

    consensus_without_pool_test(
        cfg.clone(),
        async move |mut protocol_controller,
                    consensus_controller,
                    consensus_event_receiver,
                    selector_controller,
                    selector_receiver| {
            let start_period = 3;
            let genesis_hashes = consensus_controller
                .get_block_graph_status(None, None)
                .expect("could not get block graph status")
                .genesis_blocks;
            // create test blocks

            let t0s1 = create_block(
                Slot::new(1 + start_period, 0),
                genesis_hashes.clone(),
                &staking_key,
            );

            let t1s1 = create_block(
                Slot::new(1 + start_period, 1),
                genesis_hashes.clone(),
                &staking_key,
            );

            let t0s2 = create_block(
                Slot::new(2 + start_period, 0),
                vec![t0s1.id, t1s1.id],
                &staking_key,
            );
            let t1s2 = create_block(
                Slot::new(2 + start_period, 1),
                vec![t0s1.id, t1s1.id],
                &staking_key,
            );

            let t0s3 = create_block(
                Slot::new(3 + start_period, 0),
                vec![t0s2.id, t1s2.id],
                &staking_key,
            );
            let t1s3 = create_block(
                Slot::new(3 + start_period, 1),
                vec![t0s2.id, t1s2.id],
                &staking_key,
            );

            let t0s4 = create_block(
                Slot::new(4 + start_period, 0),
                vec![t0s3.id, t1s3.id],
                &staking_key,
            );
            let t1s4 = create_block(
                Slot::new(4 + start_period, 1),
                vec![t0s3.id, t1s3.id],
                &staking_key,
            );

            // send blocks  t0s1, t1s1,
            register_block(
                &consensus_controller,
                &selector_receiver,
                t0s1.clone(),
                storage.clone(),
            );
            register_block(
                &consensus_controller,
                &selector_receiver,
                t1s1.clone(),
                storage.clone(),
            );
            // register blocks t0s3, t1s4, t0s4, t0s2, t1s3, t1s2
            register_block(
                &consensus_controller,
                &selector_receiver,
                t0s3.clone(),
                storage.clone(),
            );
            register_block(
                &consensus_controller,
                &selector_receiver,
                t1s4.clone(),
                storage.clone(),
            );
            register_block(
                &consensus_controller,
                &selector_receiver,
                t0s4.clone(),
                storage.clone(),
            );
            register_block(
                &consensus_controller,
                &selector_receiver,
                t0s2.clone(),
                storage.clone(),
            );
            register_block(
                &consensus_controller,
                &selector_receiver,
                t1s3.clone(),
                storage.clone(),
            );
            register_block(
                &consensus_controller,
                &selector_receiver,
                t1s2.clone(),
                storage.clone(),
            );

            // block t0s1 and t1s1 are propagated
            let staking_address = Address::from_public_key(&staking_key.get_public_key());
            let hash_list = vec![t0s1.id, t1s1.id];
            answer_ask_producer_pos(
                &selector_receiver,
                &staking_address,
                3000 + start_period * 1000,
            );
            answer_ask_selection_pos(&selector_receiver, &staking_address, 1000);
            validate_propagate_block_in_list(&mut protocol_controller, &hash_list, 1000).await;

            answer_ask_producer_pos(&selector_receiver, &staking_address, 1000);
            answer_ask_selection_pos(&selector_receiver, &staking_address, 1000);
            validate_propagate_block_in_list(&mut protocol_controller, &hash_list, 1000).await;
            // block t0s2 and t1s2 are propagated
            let hash_list = vec![t0s2.id, t1s2.id];
            answer_ask_producer_pos(&selector_receiver, &staking_address, 1000);
            answer_ask_selection_pos(&selector_receiver, &staking_address, 1000);
            validate_propagate_block_in_list(&mut protocol_controller, &hash_list, 1000).await;
            answer_ask_producer_pos(&selector_receiver, &staking_address, 1000);
            answer_ask_selection_pos(&selector_receiver, &staking_address, 1000);
            validate_propagate_block_in_list(&mut protocol_controller, &hash_list, 1000).await;
            // block t0s3 and t1s3 are propagated
            let hash_list = vec![t0s3.id, t1s3.id];
            answer_ask_producer_pos(&selector_receiver, &staking_address, 1000);
            answer_ask_selection_pos(&selector_receiver, &staking_address, 1000);
            validate_propagate_block_in_list(&mut protocol_controller, &hash_list, 1000).await;
            answer_ask_producer_pos(&selector_receiver, &staking_address, 1000);
            answer_ask_selection_pos(&selector_receiver, &staking_address, 1000);
            validate_propagate_block_in_list(&mut protocol_controller, &hash_list, 1000).await;
            // block t0s4 and t1s4 are propagated
            let hash_list = vec![t0s4.id, t1s4.id];
            answer_ask_producer_pos(&selector_receiver, &staking_address, 1000);
            answer_ask_selection_pos(&selector_receiver, &staking_address, 1000);
            validate_propagate_block_in_list(&mut protocol_controller, &hash_list, 1000).await;
            answer_ask_producer_pos(&selector_receiver, &staking_address, 1000);
            answer_ask_selection_pos(&selector_receiver, &staking_address, 1000);
            validate_propagate_block_in_list(&mut protocol_controller, &hash_list, 4000).await;
            (
                protocol_controller,
                consensus_controller,
                consensus_event_receiver,
                selector_controller,
                selector_receiver,
            )
        },
    )
    .await;
}

#[tokio::test]
async fn test_grandpa_incompatibility() {
    let staking_key: KeyPair = KeyPair::generate();
    let cfg = ConsensusConfig {
        t0: 200.into(),
        thread_count: 2,
        genesis_timestamp: MassaTime::now().unwrap(),
        future_block_processing_max_periods: 50,
        force_keep_final_periods: 10,
        delta_f0: 32,
        ..ConsensusConfig::default()
    };
    let storage = Storage::create_root();
    let staking_address = Address::from_public_key(&staking_key.get_public_key());

    consensus_without_pool_test(
        cfg.clone(),
        async move |protocol_controller,
                    consensus_controller,
                    consensus_event_receiver,
                    selector_controller,
                    selector_receiver| {
            let genesis = consensus_controller
                .get_block_graph_status(None, None)
                .expect("could not get block graph status")
                .genesis_blocks;

            let block_1 = create_block(Slot::new(1, 0), vec![genesis[0], genesis[1]], &staking_key);
            register_block(
                &consensus_controller,
                &selector_receiver,
                block_1.clone(),
                storage.clone(),
            );
            answer_ask_producer_pos(&selector_receiver, &staking_address, 1000);
            answer_ask_selection_pos(&selector_receiver, &staking_address, 1000);
            let block_2 = create_block(Slot::new(1, 1), vec![genesis[0], genesis[1]], &staking_key);
            register_block(
                &consensus_controller,
                &selector_receiver,
                block_2.clone(),
                storage.clone(),
            );
            answer_ask_producer_pos(&selector_receiver, &staking_address, 1000);
            answer_ask_selection_pos(&selector_receiver, &staking_address, 1000);

            let block_3 = create_block(Slot::new(2, 0), vec![block_1.id, genesis[1]], &staking_key);
            register_block(
                &consensus_controller,
                &selector_receiver,
                block_3.clone(),
                storage.clone(),
            );
            answer_ask_producer_pos(&selector_receiver, &staking_address, 1000);
            answer_ask_selection_pos(&selector_receiver, &staking_address, 1000);

            let block_4 = create_block(Slot::new(2, 1), vec![genesis[0], block_2.id], &staking_key);
            register_block(
                &consensus_controller,
                &selector_receiver,
                block_4.clone(),
                storage.clone(),
            );
            answer_ask_producer_pos(&selector_receiver, &staking_address, 1000);
            answer_ask_selection_pos(&selector_receiver, &staking_address, 1000);

            let status = consensus_controller
                .get_block_graph_status(None, None)
                .expect("could not get block graph status");
            assert!(if let Some(h) = status.gi_head.get(&block_4.id) {
                h.contains(&block_3.id)
            } else {
                panic!("missing block in gi_head")
            });

            assert_eq!(status.max_cliques.len(), 2);

            for clique in status.max_cliques.clone() {
                if clique.block_ids.contains(&block_3.id) && clique.block_ids.contains(&block_4.id)
                {
                    panic!("incompatible blocks in the same clique")
                }
            }

            let parents: Vec<BlockId> = status.best_parents.iter().map(|(b, _p)| *b).collect();
            if block_4.id > block_3.id {
                assert_eq!(parents[0], block_3.id)
            } else {
                assert_eq!(parents[1], block_4.id)
            }

            let mut latest_extra_blocks = VecDeque::new();
            for extend_i in 0..33 {
                let status = consensus_controller
                    .get_block_graph_status(None, None)
                    .expect("could not get block graph status");
                let block = create_block(
                    Slot::new(3 + extend_i, 0),
                    status.best_parents.iter().map(|(b, _p)| *b).collect(),
                    &staking_key,
                );
                register_block(
                    &consensus_controller,
                    &selector_receiver,
                    block.clone(),
                    storage.clone(),
                );
                answer_ask_producer_pos(&selector_receiver, &staking_address, 1000);
                answer_ask_selection_pos(&selector_receiver, &staking_address, 1000);
                latest_extra_blocks.push_back(block.id);
                while latest_extra_blocks.len() > cfg.delta_f0 as usize + 1 {
                    latest_extra_blocks.pop_front();
                }
            }

            let latest_extra_blocks: HashSet<BlockId> = latest_extra_blocks.into_iter().collect();
            let status = consensus_controller
                .get_block_graph_status(None, None)
                .expect("could not get block graph status");
            assert_eq!(status.max_cliques.len(), 1, "wrong cliques (len)");
            assert_eq!(
                status.max_cliques[0]
                    .block_ids
                    .iter()
                    .cloned()
                    .collect::<HashSet<BlockId>>(),
                latest_extra_blocks,
                "wrong cliques"
            );

            (
                protocol_controller,
                consensus_controller,
                consensus_event_receiver,
                selector_controller,
                selector_receiver,
            )
        },
    )
    .await;
}
