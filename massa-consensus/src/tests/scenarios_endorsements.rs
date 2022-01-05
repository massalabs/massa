// Copyright (c) 2021 MASSA LABS <info@massa.net>

use massa_hash::hash::Hash;
use massa_models::{
    Address, Amount, BlockId, Endorsement, EndorsementContent, SerializeCompact, Slot,
};
use massa_signature::{derive_public_key, generate_random_private_key, sign};
use massa_time::MassaTime;
use serial_test::serial;
use std::collections::HashMap;
use std::str::FromStr;

use crate::tests::tools::{self, create_block, generate_ledger_file};

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
    let thread_count = 2;
    // define addresses use for the test
    // addresses 1 and 2 both in thread 0
    let mut priv_1 = generate_random_private_key();
    let mut pubkey_1 = derive_public_key(&priv_1);
    let mut address_1 = Address::from_public_key(&pubkey_1);
    while 0 != address_1.get_thread(thread_count) {
        priv_1 = generate_random_private_key();
        pubkey_1 = derive_public_key(&priv_1);
        address_1 = Address::from_public_key(&pubkey_1);
    }
    assert_eq!(0, address_1.get_thread(thread_count));

    let mut priv_2 = generate_random_private_key();
    let mut pubkey_2 = derive_public_key(&priv_2);
    let mut address_2 = Address::from_public_key(&pubkey_2);
    while 0 != address_2.get_thread(thread_count) {
        priv_2 = generate_random_private_key();
        pubkey_2 = derive_public_key(&priv_2);
        address_2 = Address::from_public_key(&pubkey_2);
    }
    assert_eq!(0, address_2.get_thread(thread_count));

    let ledger_file = generate_ledger_file(&HashMap::new());

    let roll_counts_file = tools::generate_default_roll_counts_file(vec![priv_1, priv_2]);

    let staking_file = tools::generate_staking_keys_file(&vec![]);
    let mut cfg = tools::default_consensus_config(
        ledger_file.path(),
        roll_counts_file.path(),
        staking_file.path(),
    );
    cfg.periods_per_cycle = 2;
    cfg.pos_lookback_cycles = 2;
    cfg.pos_lock_cycles = 1;
    cfg.t0 = 500.into();
    cfg.delta_f0 = 3;
    cfg.disable_block_creation = true;
    cfg.thread_count = thread_count;
    cfg.block_reward = Amount::default();
    cfg.roll_price = Amount::from_str("1000").unwrap();
    cfg.operation_validity_periods = 100;
    cfg.genesis_timestamp = MassaTime::now().unwrap().saturating_add(300.into());
    cfg.endorsement_count = 1;

    tools::consensus_without_pool_test(
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

            tools::propagate_block(&mut protocol_controller, b10, false, 500).await;

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

            tools::propagate_block(&mut protocol_controller, b10, false, 500).await;

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

            tools::propagate_block(&mut protocol_controller, b10, false, 500).await;

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

            tools::propagate_block(&mut protocol_controller, b10, false, 500).await;

            (
                protocol_controller,
                consensus_command_sender,
                consensus_event_receiver,
            )
        },
    )
    .await;
}
