// Copyright (c) 2022 MASSA LABS <info@massa.net>

use massa_models::{
    amount::Amount,
    block::BlockId,
    endorsement::{Endorsement, EndorsementSerializer},
    slot::Slot,
    wrapped::WrappedContent,
};
use massa_signature::KeyPair;
use massa_storage::Storage;
use massa_time::MassaTime;
use serial_test::serial;
use std::str::FromStr;

use super::tools::*;
use massa_consensus_exports::{test_exports::generate_default_roll_counts_file, ConsensusConfig};

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
        genesis_timestamp: MassaTime::now(0).unwrap().saturating_add(300.into()),
        operation_validity_periods: 100,
        periods_per_cycle: 2,
        roll_price: Amount::from_str("1000").unwrap(),
        t0: 500.into(),
        ..ConsensusConfig::default_with_paths()
    };
    // define addresses use for the test
    // addresses 1 and 2 both in thread 0

    let (address_1, keypair_1) = random_address_on_thread(0, cfg.thread_count).into();
    let (address_2, keypair_2) = random_address_on_thread(0, cfg.thread_count).into();
    assert_eq!(0, address_2.get_thread(cfg.thread_count));
    let initial_rolls_file =
        generate_default_roll_counts_file(vec![keypair_1.clone(), keypair_2.clone()]);
    cfg.initial_rolls_path = initial_rolls_file.path().to_path_buf();

    let mut storage = Storage::create_root();
    consensus_without_pool_test(
        cfg.clone(),
        async move |mut protocol_controller,
                    consensus_command_sender,
                    consensus_event_receiver,
                    selector_controller| {
            let address_a = selector_controller
                .get_selection(Slot::new(1, 0))
                .unwrap()
                .producer;
            let address_b = selector_controller
                .get_selection(Slot::new(1, 0))
                .unwrap()
                .endorsements[0];
            let address_c = selector_controller
                .get_selection(Slot::new(1, 1))
                .unwrap()
                .endorsements[1];

            let keypair_a = if address_a == address_1 {
                keypair_1.clone()
            } else {
                keypair_2.clone()
            };
            let keypair_b = if address_b == address_1 {
                keypair_1.clone()
            } else {
                keypair_2.clone()
            };
            let keypair_c = if address_c == address_1 {
                keypair_1.clone()
            } else {
                keypair_2.clone()
            };

            let parents: Vec<BlockId> = consensus_command_sender
                .get_block_graph_status(None, None)
                .await
                .unwrap()
                .best_parents
                .iter()
                .map(|(b, _p)| *b)
                .collect();

            let mut b10 = create_block(&cfg, Slot::new(1, 0), parents.clone(), &keypair_a);

            // create an otherwise valid endorsement with another address, include it in valid block(1,0), assert it is not propagated
            let sender_keypair = KeyPair::generate();
            let content = Endorsement {
                slot: Slot::new(1, 0),
                index: 0,
                endorsed_block: parents[0],
            };
            let ed = Endorsement::new_wrapped(
                content.clone(),
                EndorsementSerializer::new(),
                &sender_keypair,
            )
            .unwrap();
            b10.content.header.content.endorsements = vec![ed];

            storage.store_block(b10.clone());
            propagate_block(
                &mut protocol_controller,
                b10.id,
                b10.content.header.content.slot,
                storage.clone(),
                false,
                500,
            )
            .await;

            // create an otherwise valid endorsement at slot (1,1), include it in valid block(1,0), assert it is not propagated
            let content = Endorsement {
                slot: Slot::new(1, 1),
                index: 0,
                endorsed_block: parents[1],
            };
            let ed =
                Endorsement::new_wrapped(content.clone(), EndorsementSerializer::new(), &keypair_c)
                    .unwrap();
            let mut b10 = create_block(&cfg, Slot::new(1, 0), parents.clone(), &keypair_a);
            b10.content.header.content.endorsements = vec![ed];

            storage.store_block(b10.clone());
            propagate_block(
                &mut protocol_controller,
                b10.id,
                b10.content.header.content.slot,
                storage.clone(),
                false,
                500,
            )
            .await;

            // create an otherwise valid endorsement with genesis 1 as endorsed block, include it in valid block(1,0), assert it is not propagated
            let content = Endorsement {
                slot: Slot::new(1, 0),
                index: 0,
                endorsed_block: parents[1],
            };
            let ed =
                Endorsement::new_wrapped(content.clone(), EndorsementSerializer::new(), &keypair_b)
                    .unwrap();
            let mut b10 = create_block(&cfg, Slot::new(1, 0), parents.clone(), &keypair_a);
            b10.content.header.content.endorsements = vec![ed];

            storage.store_block(b10.clone());
            propagate_block(
                &mut protocol_controller,
                b10.id,
                b10.content.header.content.slot,
                storage.clone(),
                false,
                500,
            )
            .await;

            // create a valid endorsement, include it in valid block(1,1), assert it is propagated
            let content = Endorsement {
                slot: Slot::new(1, 0),
                index: 0,
                endorsed_block: parents[0],
            };
            let ed =
                Endorsement::new_wrapped(content.clone(), EndorsementSerializer::new(), &keypair_b)
                    .unwrap();
            let mut b10 = create_block(&cfg, Slot::new(1, 0), parents.clone(), &keypair_a);
            b10.content.header.content.endorsements = vec![ed];

            storage.store_block(b10.clone());
            propagate_block(
                &mut protocol_controller,
                b10.id,
                b10.content.header.content.slot,
                storage.clone(),
                false,
                500,
            )
            .await;

            (
                protocol_controller,
                consensus_command_sender,
                consensus_event_receiver,
                selector_controller,
            )
        },
    )
    .await;
}
