// Copyright (c) 2022 MASSA LABS <info@massa.net>

use massa_hash::hash::Hash;
use massa_models::{Amount, BlockId, Endorsement, EndorsementContent, SerializeCompact, Slot};
use massa_signature::{derive_public_key, generate_random_private_key, sign};
use massa_time::MassaTime;
use serial_test::serial;
use std::{collections::HashMap, str::FromStr};

use super::tools::*;
use massa_consensus_exports::{tools::*, ConsensusConfig};

#[tokio::test]
#[serial]
async fn test_endorsement_check() {
    // setup logging
    /*
    stderrlog::new()
        .verbosity(4)
        .timestamp(stderrlog::Timestamp::Millisecond)
        .init()
        .unwrap();
    */
    let mut cfg = ConsensusConfig {
        block_reward: Amount::default(),
        delta_f0: 3,
        endorsement_count: 1,
        genesis_timestamp: MassaTime::now().unwrap().saturating_add(300.into()),
        operation_validity_periods: 100,
        periods_per_cycle: 2,
        pos_lock_cycles: 1,
        roll_price: Amount::from_str("1000").unwrap(),
        t0: 500.into(),
        ..ConsensusConfig::default_with_paths()
    };
    // define addresses use for the test
    // addresses 1 and 2 both in thread 0

    let (address_1, priv_1, pubkey_1) = random_address_on_thread(0, cfg.thread_count).into();
    let (address_2, priv_2, pubkey_2) = random_address_on_thread(0, cfg.thread_count).into();
    assert_eq!(0, address_2.get_thread(cfg.thread_count));
    let initial_rolls_file = generate_default_roll_counts_file(vec![priv_1, priv_2]);
    cfg.initial_rolls_path = initial_rolls_file.path().to_path_buf();

    consensus_without_pool_test(
        cfg.clone(),
        async move |mut protocol_controller, consensus_command_sender, consensus_event_receiver| {
            let draws: HashMap<_, _> = consensus_command_sender
                .get_selection_draws(Slot::new(1, 0), Slot::new(2, 0))
                .await
                .unwrap()
                .into_iter()
                .collect();

            let address_a = draws.get(&Slot::new(1, 0)).unwrap().0;
            let address_b = draws.get(&Slot::new(1, 0)).unwrap().1[0];
            let address_c = draws.get(&Slot::new(1, 1)).unwrap().1[0];

            let (_pub_key_a, priv_key_a) = if address_a == address_1 {
                (pubkey_1, priv_1)
            } else {
                (pubkey_2, priv_2)
            };
            let (pub_key_b, _priv_key_b) = if address_b == address_1 {
                (pubkey_1, priv_1)
            } else {
                (pubkey_2, priv_2)
            };
            let (pub_key_c, _priv_key_c) = if address_c == address_1 {
                (pubkey_1, priv_1)
            } else {
                (pubkey_2, priv_2)
            };

            let parents: Vec<BlockId> = consensus_command_sender
                .get_block_graph_status(None, None)
                .await
                .unwrap()
                .best_parents
                .iter()
                .map(|(b, _p)| *b)
                .collect();

            let (_, mut b10, _) = create_block(&cfg, Slot::new(1, 0), parents.clone(), priv_key_a);

            // create an otherwise valid endorsement with another address, include it in valid block(1,0), assert it is not propagated
            let sender_priv = generate_random_private_key();
            let sender_public_key = derive_public_key(&sender_priv);
            let content = EndorsementContent {
                sender_public_key,
                slot: Slot::new(1, 0),
                index: 0,
                endorsed_block: parents[0],
            };
            let hash = Hash::compute_from(&content.to_bytes_compact().unwrap());
            let signature = sign(&hash, &sender_priv).unwrap();
            let ed = Endorsement {
                content: content.clone(),
                signature,
            };

            b10.header.content.endorsements = vec![ed];

            propagate_block(&mut protocol_controller, b10, false, 500).await;

            // create an otherwise valid endorsement at slot (1,1), include it in valid block(1,0), assert it is not propagated
            let content = EndorsementContent {
                sender_public_key: pub_key_c,
                slot: Slot::new(1, 1),
                index: 0,
                endorsed_block: parents[1],
            };
            let hash = Hash::compute_from(&content.to_bytes_compact().unwrap());
            let signature = sign(&hash, &sender_priv).unwrap();
            let ed = Endorsement {
                content: content.clone(),
                signature,
            };
            let (_, mut b10, _) = create_block(&cfg, Slot::new(1, 0), parents.clone(), priv_key_a);
            b10.header.content.endorsements = vec![ed];

            propagate_block(&mut protocol_controller, b10, false, 500).await;

            // create an otherwise valid endorsement with genesis 1 as endorsed block, include it in valid block(1,0), assert it is not propagated
            let content = EndorsementContent {
                sender_public_key: pub_key_b,
                slot: Slot::new(1, 0),
                index: 0,
                endorsed_block: parents[1],
            };
            let hash = Hash::compute_from(&content.to_bytes_compact().unwrap());
            let signature = sign(&hash, &sender_priv).unwrap();
            let ed = Endorsement {
                content: content.clone(),
                signature,
            };
            let (_, mut b10, _) = create_block(&cfg, Slot::new(1, 0), parents.clone(), priv_key_a);
            b10.header.content.endorsements = vec![ed];

            propagate_block(&mut protocol_controller, b10, false, 500).await;

            // create a valid endorsement, include it in valid block(1,1), assert it is propagated
            let content = EndorsementContent {
                sender_public_key: pub_key_b,
                slot: Slot::new(1, 0),
                index: 0,
                endorsed_block: parents[0],
            };
            let hash = Hash::compute_from(&content.to_bytes_compact().unwrap());
            let signature = sign(&hash, &sender_priv).unwrap();
            let ed = Endorsement {
                content: content.clone(),
                signature,
            };
            let (_, mut b10, _) = create_block(&cfg, Slot::new(1, 0), parents.clone(), priv_key_a);
            b10.header.content.endorsements = vec![ed];

            propagate_block(&mut protocol_controller, b10, false, 500).await;

            (
                protocol_controller,
                consensus_command_sender,
                consensus_event_receiver,
            )
        },
    )
    .await;
}
