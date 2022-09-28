use crate::tests::tools::get_dummy_block_id;
use massa_graph::{
    export_active_block::ExportActiveBlock, BootstrapableGraph, BootstrapableGraphDeserializer,
    BootstrapableGraphSerializer,
};
use massa_hash::Hash;
use massa_models::{
    block::{Block, BlockHeader, BlockHeaderSerializer, BlockSerializer, WrappedBlock},
    endorsement::{Endorsement, EndorsementSerializerLW},
    slot::Slot,
    wrapped::WrappedContent,
};

use massa_serialization::{DeserializeError, Deserializer, Serializer};
use massa_signature::KeyPair;
use serial_test::serial;

/// the data input to create the public keys was generated using the secp256k1 curve
/// a test using this function is a regression test not an implementation test
fn get_export_active_test_block() -> (WrappedBlock, ExportActiveBlock) {
    let keypair = KeyPair::generate();
    let block = Block::new_wrapped(
        Block {
            header: BlockHeader::new_wrapped(
                BlockHeader {
                    operation_merkle_root: Hash::compute_from(&Vec::new()),
                    parents: vec![get_dummy_block_id("parent1"), get_dummy_block_id("parent2")],
                    slot: Slot::new(1, 0),
                    endorsements: vec![Endorsement::new_wrapped(
                        Endorsement {
                            endorsed_block: get_dummy_block_id("parent1"),
                            index: 0,
                            slot: Slot::new(1, 0),
                        },
                        EndorsementSerializerLW::new(),
                        &keypair,
                    )
                    .unwrap()],
                },
                BlockHeaderSerializer::new(),
                &keypair,
            )
            .unwrap(),
            operations: Default::default(),
        },
        BlockSerializer::new(),
        &keypair,
    )
    .unwrap();

    (
        block.clone(),
        ExportActiveBlock {
            parents: vec![
                (get_dummy_block_id("parent11"), 23),
                (get_dummy_block_id("parent12"), 24),
            ],
            block,
            operations: vec![],
            is_final: true,
        },
    )
}

#[test]
#[serial]
fn test_bootstrapable_graph_serialized() {
    //let storage: Storage = Storage::create_root();

    let (_, active_block) = get_export_active_test_block();

    //storage.store_block(block.header.content.compute_id().expect("Fail to calculate block id."), block, block.to_bytes_compact().expect("Fail to serialize block"));

    let graph = BootstrapableGraph {
        /// Map of active blocks, were blocks are in their exported version.
        final_blocks: vec![active_block].into_iter().collect(),
    };

    let bootstrapable_graph_serializer = BootstrapableGraphSerializer::new();
    let bootstrapable_graph_deserializer = BootstrapableGraphDeserializer::new(
        2, 8, 10000, 10000, 10000, 10000, 10000, 10, 255, 10_000,
    );
    let mut bytes = Vec::new();

    bootstrapable_graph_serializer
        .serialize(&graph, &mut bytes)
        .unwrap();
    let (_, new_graph) = bootstrapable_graph_deserializer
        .deserialize::<DeserializeError>(&bytes)
        .unwrap();

    assert_eq!(
        graph.final_blocks[0].block.serialized_data,
        new_graph.final_blocks[0].block.serialized_data
    );
}

// #[tokio::test]
// #[serial]
// async fn test_clique_calculation() {
//     let ledger_file = generate_ledger_file(&Map::default());
//     let cfg = ConsensusConfig::from(ledger_file.path());
//     let storage: Storage = Storage::create_root();
//     let selector_config = SelectorConfig {
//         initial_rolls_path: cfg.initial_rolls_path.clone(),
//         thread_count: 2,
//         periods_per_cycle: 100,
//         genesis_address: Address::from_str("A12hgh5ULW9o8fJE9muLNXhQENaUUswQbxPyDSq8ridnDGu5gRiJ")
//             .unwrap(),
//         endorsement_count: 0,
//         max_draw_cache: 10,
//         initial_draw_seed: "".to_string(),
//     };
//     let (mut selector_manager, selector_controller) =
//         start_selector_worker(selector_config).unwrap();
//     let mut block_graph =
//         BlockGraph::new(GraphConfig::from(&cfg), None, storage, selector_controller)
//             .await
//             .unwrap();
//     let hashes: Vec<BlockId> = vec![
//         "VzCRpnoZVYY1yQZTXtVQbbxwzdu6hYtdCUZB5BXWSabsiXyfP",
//         "JnWwNHRR1tUD7UJfnEFgDB4S4gfDTX2ezLadr7pcwuZnxTvn1",
//         "xtvLedxC7CigAPytS5qh9nbTuYyLbQKCfbX8finiHsKMWH6SG",
//         "2Qs9sSbc5sGpVv5GnTeDkTKdDpKhp4AgCVT4XFcMaf55msdvJN",
//         "2VNc8pR4tNnZpEPudJr97iNHxXbHiubNDmuaSzrxaBVwKXxV6w",
//         "2bsrYpfLdvVWAJkwXoJz1kn4LWshdJ6QjwTrA7suKg8AY3ecH1",
//         "kfUeGj3ZgBprqFRiAQpE47dW5tcKTAueVaWXZquJW6SaPBd4G",
//     ]
//     .into_iter()
//     .map(|h| BlockId::from_bs58_check(h).unwrap())
//     .collect();
//     block_graph.gi_head = vec![
//         (0, vec![1, 2, 3, 4]),
//         (1, vec![0]),
//         (2, vec![0]),
//         (3, vec![0]),
//         (4, vec![0]),
//         (5, vec![6]),
//         (6, vec![5]),
//     ]
//     .into_iter()
//     .map(|(idx, lst)| (hashes[idx], lst.into_iter().map(|i| hashes[i]).collect()))
//     .collect();
//     let computed_sets = block_graph.compute_max_cliques();

//     let expected_sets: Vec<Set<BlockId>> = vec![
//         vec![1, 2, 3, 4, 5],
//         vec![1, 2, 3, 4, 6],
//         vec![0, 5],
//         vec![0, 6],
//     ]
//     .into_iter()
//     .map(|lst| lst.into_iter().map(|i| hashes[i]).collect())
//     .collect();

//     assert_eq!(computed_sets.len(), expected_sets.len());
//     for expected in expected_sets.into_iter() {
//         assert!(computed_sets.iter().any(|v| v == &expected));
//     }
//     selector_manager.stop();
// }

// /// generate a named temporary JSON ledger file
// fn generate_ledger_file(ledger_vec: &Map<Address, LedgerData>) -> NamedTempFile {
//     use std::io::prelude::*;
//     let ledger_file_named = NamedTempFile::new().expect("cannot create temp file");
//     serde_json::to_writer_pretty(ledger_file_named.as_file(), &ledger_vec)
//         .expect("unable to write ledger file");
//     ledger_file_named
//         .as_file()
//         .seek(std::io::SeekFrom::Start(0))
//         .expect("could not seek file");
//     ledger_file_named
// }
