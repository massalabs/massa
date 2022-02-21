use crate::tests::tools::get_dummy_block_id;
use massa_consensus_exports::ConsensusConfig;
use massa_graph::{
    create_genesis_block, export_active_block::ExportActiveBlock, ledger::LedgerSubset,
    settings::GraphConfig, BlockGraph, BootstrapableGraph,
};
use massa_hash::hash::Hash;
use massa_models::{
    active_block::ActiveBlock,
    clique::Clique,
    ledger_models::{LedgerChange, LedgerChanges, LedgerData},
    prehash::{Map, Set},
    Address, Block, BlockHeader, BlockHeaderContent, BlockId, DeserializeCompact, SerializeCompact,
    Slot,
};
use massa_models::{Amount, Endorsement, EndorsementContent};
use massa_signature::{PublicKey, Signature};
use serial_test::serial;
use std::str::FromStr;
use tempfile::NamedTempFile;

fn get_export_active_test_block() -> ExportActiveBlock {
    let block = Block {
            header: BlockHeader {
                content: BlockHeaderContent{
                    creator: PublicKey::from_bs58_check("4vYrPNzUM8PKg2rYPW3ZnXPzy67j9fn5WsGCbnwAnk2Lf7jNHb").unwrap(),
                    operation_merkle_root: Hash::compute_from(&Vec::new()),
                    parents: vec![
                        get_dummy_block_id("parent1"),
                        get_dummy_block_id("parent2"),
                    ],
                    slot: Slot::new(1, 0),
                    endorsements: vec![ Endorsement{content: EndorsementContent{
                        sender_public_key: PublicKey::from_bs58_check("4vYrPNzUM8PKg2rYPW3ZnXPzy67j9fn5WsGCbnwAnk2Lf7jNHb").unwrap(),
                        endorsed_block: get_dummy_block_id("parent1"),
                        index: 0,
                        slot: Slot::new(1, 0),
                    }, signature: Signature::from_bs58_check(
                        "5f4E3opXPWc3A1gvRVV7DJufvabDfaLkT1GMterpJXqRZ5B7bxPe5LoNzGDQp9LkphQuChBN1R5yEvVJqanbjx7mgLEae"
                    ).unwrap() }],
                },
                signature: Signature::from_bs58_check(
                    "5f4E3opXPWc3A1gvRVV7DJufvabDfaLkT1GMterpJXqRZ5B7bxPe5LoNzGDQp9LkphQuChBN1R5yEvVJqanbjx7mgLEae"
                ).unwrap()
            },
            operations: vec![]
        };

    ExportActiveBlock {
        parents: vec![
            (get_dummy_block_id("parent11"), 23),
            (get_dummy_block_id("parent12"), 24),
        ],
        dependencies: vec![
            get_dummy_block_id("dep11"),
            get_dummy_block_id("dep12"),
            get_dummy_block_id("dep13"),
        ]
        .into_iter()
        .collect(),
        block,
        children: vec![vec![
            (get_dummy_block_id("child11"), 31),
            (get_dummy_block_id("child11"), 31),
        ]
        .into_iter()
        .collect()],
        is_final: true,
        block_ledger_changes: LedgerChanges(
            vec![
                (
                    Address::from_bytes(&Hash::compute_from("addr01".as_bytes()).into_bytes())
                        .unwrap(),
                    LedgerChange {
                        balance_delta: Amount::from_str("1").unwrap(),
                        balance_increment: true, // whether to increment or decrement balance of delta
                    },
                ),
                (
                    Address::from_bytes(&Hash::compute_from("addr02".as_bytes()).into_bytes())
                        .unwrap(),
                    LedgerChange {
                        balance_delta: Amount::from_str("2").unwrap(),
                        balance_increment: false, // whether to increment or decrement balance of delta
                    },
                ),
                (
                    Address::from_bytes(&Hash::compute_from("addr11".as_bytes()).into_bytes())
                        .unwrap(),
                    LedgerChange {
                        balance_delta: Amount::from_str("3").unwrap(),
                        balance_increment: false, // whether to increment or decrement balance of delta
                    },
                ),
            ]
            .into_iter()
            .collect(),
        ),
        roll_updates: Default::default(),
        production_events: vec![],
    }
}

#[tokio::test]
#[serial]
pub async fn test_get_ledger_at_parents() {
    //     stderrlog::new()
    // .verbosity(4)
    // .timestamp(stderrlog::Timestamp::Millisecond)
    // .init()
    // .unwrap();
    let thread_count: u8 = 2;
    let active_block: ActiveBlock = get_export_active_test_block().try_into().unwrap();
    let ledger_file = generate_ledger_file(&Map::default());
    let mut cfg = ConsensusConfig::from(ledger_file.path());
    cfg.thread_count = thread_count;
    cfg.block_reward = Amount::from_str("1").unwrap();
    // to generate address and public keys
    /*        let private_key = generate_random_private_key();
    let public_key = derive_public_key(&private_key);

    let add = Address::from_public_key(&public_key);

    println!(
        "public key:{}, address:{}, th:{}",
        public_key.to_bs58_check(),
        add.to_bs58_check(),
        add.get_thread(thread_count)
    ); */

    // define addresses use for the test
    let pubkey_a =
        PublicKey::from_bs58_check("5UvFn66yoQerrEmikCxDVvhkLvCo9R2hJAYFMh2pZfYUQDMuCE").unwrap();
    let address_a = Address::from_public_key(&pubkey_a);
    assert_eq!(0, address_a.get_thread(thread_count));

    let pubkey_b =
        PublicKey::from_bs58_check("4uRbkzUvQwW19dD6cxQ9WiYo8BZTPQsmsCbBrFLxMiUYTSbo2p").unwrap();
    let address_b = Address::from_public_key(&pubkey_b);
    assert_eq!(1, address_b.get_thread(thread_count));

    let address_c =
        Address::from_bs58_check("2cABaQpb4fgYjGE7z2TnbQ2DePsyh9KwwPbodS7fD9Pft9uS1p").unwrap();
    assert_eq!(1, address_c.get_thread(thread_count));
    let address_d =
        Address::from_bs58_check("21bU2xruH7bFzfcUhJ6SGjnLmC9cMt1kxzqFr11eV58uj7Ui8h").unwrap();
    assert_eq!(1, address_d.get_thread(thread_count));

    let graph_cfg = GraphConfig::from(&cfg);
    let (hash_genesist0, block_genesist0) = create_genesis_block(&graph_cfg, 0).unwrap();
    let (hash_genesist1, block_genesist1) = create_genesis_block(&graph_cfg, 1).unwrap();
    let export_genesist0 = ExportActiveBlock {
        block: block_genesist0,
        parents: vec![],  // one (hash, period) per thread ( if not genesis )
        children: vec![], // one HashMap<hash, period> per thread (blocks that need to be kept)
        dependencies: Default::default(), // dependencies required for validity check
        is_final: true,
        block_ledger_changes: Default::default(),
        roll_updates: Default::default(),
        production_events: vec![],
    };
    let export_genesist1 = ExportActiveBlock {
        block: block_genesist1,
        parents: vec![],  // one (hash, period) per thread ( if not genesis )
        children: vec![], // one HashMap<hash, period> per thread (blocks that need to be kept)
        dependencies: Default::default(), // dependencies required for validity check
        is_final: true,
        block_ledger_changes: Default::default(),
        roll_updates: Default::default(),
        production_events: vec![],
    };
    // update ledger with initial content.
    //   Thread 0  [at the output of block p0t0]:
    //   A 1000000000
    // Thread 1 [at the output of block p2t1]:
    //   B: 2000000000

    // block reward: 1

    // create block p1t0
    // block p1t0 [NON-FINAL]: creator A, parents [p0t0, p0t1] operations:
    //   A -> B : 2, fee 4
    //   => counted as [A += +1 - 2 - 4 + 4, B += +2]
    let mut blockp1t0 = active_block.clone();
    blockp1t0.parents = vec![(hash_genesist0, 0), (hash_genesist1, 0)];
    blockp1t0.is_final = true;
    blockp1t0.block.header.content.creator = pubkey_a;
    blockp1t0.block_ledger_changes = LedgerChanges::default();
    blockp1t0
        .block_ledger_changes
        .apply(
            &address_a,
            &LedgerChange {
                balance_delta: Amount::from_str("1").unwrap(),
                balance_increment: false,
            },
        )
        .unwrap();
    blockp1t0
        .block_ledger_changes
        .apply(
            &address_b,
            &LedgerChange {
                balance_delta: Amount::from_str("2").unwrap(),
                balance_increment: true,
            },
        )
        .unwrap();
    blockp1t0.block.header.content.slot = Slot::new(1, 0);

    // block p1t1 [FINAL]: creator B, parents [p0t0, p0t1], operations:
    //   B -> A : 128, fee 64
    //   B -> A : 32, fee 16
    // => counted as [A += 128 + 32] (B: -128 -32 + 16 + 64 -16 -64 +1=-159 not counted !!)
    let mut blockp1t1 = active_block.clone();
    blockp1t1.parents = vec![(hash_genesist0, 0), (hash_genesist1, 0)];
    blockp1t1.is_final = true;
    blockp1t1.block.header.content.creator = pubkey_b;
    blockp1t1.block_ledger_changes = LedgerChanges::default();
    blockp1t1
        .block_ledger_changes
        .apply(
            &address_a,
            &LedgerChange {
                balance_delta: Amount::from_str("160").unwrap(),
                balance_increment: true,
            },
        )
        .unwrap();
    blockp1t1
        .block_ledger_changes
        .apply(
            &address_b,
            &LedgerChange {
                balance_delta: Amount::from_str("159").unwrap(),
                balance_increment: false,
            },
        )
        .unwrap();

    blockp1t1.block.header.content.slot = Slot::new(1, 1);

    // block p2t0 [NON-FINAL]: creator A, parents [p1t0, p0t1], operations:
    //   A -> A : 512, fee 1024
    // => counted as [A += 1]
    let mut blockp2t0 = active_block.clone();
    blockp2t0.parents = vec![(get_dummy_block_id("blockp1t0"), 1), (hash_genesist1, 0)];
    blockp2t0.is_final = false;
    blockp2t0.block.header.content.creator = pubkey_a;
    blockp2t0.block_ledger_changes = LedgerChanges::default();
    blockp2t0
        .block_ledger_changes
        .apply(
            &address_a,
            &LedgerChange {
                balance_delta: Amount::from_str("1").unwrap(),
                balance_increment: true,
            },
        )
        .unwrap();
    blockp2t0.block.header.content.slot = Slot::new(2, 0);

    // block p2t1 [FINAL]: creator B, parents [p1t0, p1t1] operations:
    //   B -> A : 10, fee 1
    // => counted as [A += 10] (B not counted !)
    let mut blockp2t1 = active_block.clone();
    blockp2t1.parents = vec![
        (get_dummy_block_id("blockp1t0"), 1),
        (get_dummy_block_id("blockp1t1"), 1),
    ];
    blockp2t1.is_final = true;
    blockp2t1.block.header.content.creator = pubkey_b;
    blockp2t1.block_ledger_changes = LedgerChanges::default();
    blockp2t1
        .block_ledger_changes
        .apply(
            &address_a,
            &LedgerChange {
                balance_delta: Amount::from_str("10").unwrap(),
                balance_increment: true,
            },
        )
        .unwrap();
    blockp2t1
        .block_ledger_changes
        .apply(
            &address_b,
            &LedgerChange {
                balance_delta: Amount::from_str("9").unwrap(),
                balance_increment: false,
            },
        )
        .unwrap();
    blockp2t1.block.header.content.slot = Slot::new(2, 1);

    // block p3t0 [NON-FINAL]: creator A, parents [p2t0, p1t1] operations:
    //   A -> C : 2048, fee 4096
    // => counted as [A += 1 - 2048 - 4096 (+4096) ; C created to 2048]
    let mut blockp3t0 = active_block.clone();
    blockp3t0.parents = vec![
        (get_dummy_block_id("blockp2t0"), 2),
        (get_dummy_block_id("blockp1t1"), 1),
    ];
    blockp3t0.is_final = false;
    blockp3t0.block.header.content.creator = pubkey_a;
    blockp3t0.block_ledger_changes = LedgerChanges::default();
    blockp3t0
        .block_ledger_changes
        .apply(
            &address_a,
            &LedgerChange {
                balance_delta: Amount::from_str("2047").unwrap(),
                balance_increment: false,
            },
        )
        .unwrap();
    blockp3t0
        .block_ledger_changes
        .apply(
            &address_c,
            &LedgerChange {
                balance_delta: Amount::from_str("2048").unwrap(),
                balance_increment: true,
            },
        )
        .unwrap();
    blockp3t0.block.header.content.slot = Slot::new(3, 0);

    // block p3t1 [NON-FINAL]: creator B, parents [p2t0, p2t1] operations:
    //   B -> A : 100, fee 10
    // => counted as [B += 1 - 100 - 10 + 10 ; A += 100]
    let mut blockp3t1 = active_block.clone();
    blockp3t1.parents = vec![
        (get_dummy_block_id("blockp2t0"), 2),
        (get_dummy_block_id("blockp2t1"), 2),
    ];
    blockp3t1.is_final = false;
    blockp3t1.block.header.content.creator = pubkey_b;
    blockp3t1.block_ledger_changes = LedgerChanges::default();
    blockp3t1
        .block_ledger_changes
        .apply(
            &address_a,
            &LedgerChange {
                balance_delta: Amount::from_str("100").unwrap(),
                balance_increment: true,
            },
        )
        .unwrap();
    blockp3t1
        .block_ledger_changes
        .apply(
            &address_b,
            &LedgerChange {
                balance_delta: Amount::from_str("99").unwrap(),
                balance_increment: false,
            },
        )
        .unwrap();
    blockp3t1.block.header.content.slot = Slot::new(3, 1);

    let export_graph = BootstrapableGraph {
        /// Map of active blocks, were blocks are in their exported version.
        active_blocks: vec![
            (hash_genesist0, export_genesist0),
            (hash_genesist1, export_genesist1),
            (
                get_dummy_block_id("blockp1t0"),
                ExportActiveBlock::from(&blockp1t0),
            ),
            (
                get_dummy_block_id("blockp1t1"),
                ExportActiveBlock::from(&blockp1t1),
            ),
            (
                get_dummy_block_id("blockp2t0"),
                ExportActiveBlock::from(&blockp2t0),
            ),
            (
                get_dummy_block_id("blockp2t1"),
                ExportActiveBlock::from(&blockp2t1),
            ),
            (
                get_dummy_block_id("blockp3t0"),
                ExportActiveBlock::from(&blockp3t0),
            ),
            (
                get_dummy_block_id("blockp3t1"),
                ExportActiveBlock::from(&blockp3t1),
            ),
        ]
        .into_iter()
        .collect(),
        /// Best parents hash in each thread.
        best_parents: vec![
            (get_dummy_block_id("blockp3t0"), 3),
            (get_dummy_block_id("blockp3t1"), 3),
        ],
        /// Latest final period and block hash in each thread.
        latest_final_blocks_periods: vec![
            (get_dummy_block_id("blockp1t0"), 1),
            (get_dummy_block_id("blockp2t1"), 2),
        ],
        /// Head of the incompatibility graph.
        gi_head: Default::default(),
        /// List of maximal cliques of compatible blocks.
        max_cliques: vec![],
        /// Ledger at last final blocks
        ledger: LedgerSubset(
            vec![
                (
                    address_a,
                    LedgerData {
                        balance: Amount::from_str("1000000000").unwrap(),
                    },
                ),
                (
                    address_b,
                    LedgerData {
                        balance: Amount::from_str("2000000000").unwrap(),
                    },
                ),
            ]
            .into_iter()
            .collect(),
        ),
    };

    let block_graph = BlockGraph::new(GraphConfig::from(&cfg), Some(export_graph))
        .await
        .unwrap();

    // Ledger at parents (p3t0, p3t1) for addresses A, B, C, D:
    let res = block_graph
        .get_ledger_at_parents(
            &[
                get_dummy_block_id("blockp3t0"),
                get_dummy_block_id("blockp3t1"),
            ],
            &vec![address_a, address_b, address_c, address_d]
                .into_iter()
                .collect(),
        )
        .unwrap();
    println!("res: {:#?}", res);
    // Result ledger:
    // A: 999994127
    // B: 1999999901 = 2000_000_000 - 99
    // C: 2048
    // D: 0
    assert_eq!(
        res.0[&address_a].balance,
        Amount::from_str("999998224").unwrap()
    );
    assert_eq!(
        res.0[&address_b].balance,
        Amount::from_str("1999999901").unwrap()
    );
    assert_eq!(res.0[&address_c].balance, Amount::from_str("2048").unwrap());
    assert_eq!(res.0[&address_d].balance, Amount::from_str("0").unwrap());

    // ask_ledger_at_parents for parents [p1t0, p1t1] for address A  => balance A = 1000000159
    let res = block_graph
        .get_ledger_at_parents(
            &[
                get_dummy_block_id("blockp1t0"),
                get_dummy_block_id("blockp1t1"),
            ],
            &vec![address_a].into_iter().collect(),
        )
        .unwrap();
    println!("res: {:#?}", res);
    // Result ledger:
    // A: 999994127
    // B: 1999999903
    // C: 2048
    // D: 0
    assert_eq!(
        res.0[&address_a].balance,
        Amount::from_str("1000000160").unwrap()
    );

    // ask_ledger_at_parents for parents [p1t0, p1t1] for addresses A, B => ERROR
    let res = block_graph.get_ledger_at_parents(
        &[
            get_dummy_block_id("blockp1t0"),
            get_dummy_block_id("blockp1t1"),
        ],
        &vec![address_a, address_b].into_iter().collect(),
    );
    println!("res: {:#?}", res);
    if res.is_ok() {
        panic!("get_ledger_at_parents should return an error");
    }
}

#[test]
#[serial]
fn test_bootsrapable_graph_serialize_compact() {
    // test with 2 thread
    massa_models::init_serialization_context(massa_models::SerializationContext {
        max_advertise_length: 128,
        max_bootstrap_children: 100,
        max_ask_blocks_per_message: 10,
        endorsement_count: 8,
        ..Default::default()
    });

    let active_block = get_export_active_test_block();

    let bytes = active_block.block.to_bytes_compact().unwrap();
    let new_block = Block::from_bytes_compact(&bytes).unwrap();

    println!("{:?}", new_block);
    let b1_id = get_dummy_block_id("active11");
    let graph = BootstrapableGraph {
        /// Map of active blocks, were blocks are in their exported version.
        active_blocks: vec![
            (b1_id, active_block.clone()),
            (get_dummy_block_id("active12"), active_block.clone()),
            (get_dummy_block_id("active13"), active_block),
        ]
        .into_iter()
        .collect(),
        /// Best parents hash in each thread.
        best_parents: vec![
            (get_dummy_block_id("parent11"), 2),
            (get_dummy_block_id("parent12"), 3),
        ],
        /// Latest final period and block hash in each thread.
        latest_final_blocks_periods: vec![
            (get_dummy_block_id("lfinal11"), 23),
            (get_dummy_block_id("lfinal12"), 24),
        ],
        /// Head of the incompatibility graph.
        gi_head: vec![
            (
                get_dummy_block_id("gi_head11"),
                vec![get_dummy_block_id("set11"), get_dummy_block_id("set12")]
                    .into_iter()
                    .collect(),
            ),
            (
                get_dummy_block_id("gi_head12"),
                vec![get_dummy_block_id("set21"), get_dummy_block_id("set22")]
                    .into_iter()
                    .collect(),
            ),
            (
                get_dummy_block_id("gi_head13"),
                vec![get_dummy_block_id("set31"), get_dummy_block_id("set32")]
                    .into_iter()
                    .collect(),
            ),
        ]
        .into_iter()
        .collect(),

        /// List of maximal cliques of compatible blocks.
        max_cliques: vec![Clique {
            block_ids: vec![
                get_dummy_block_id("max_cliques11"),
                get_dummy_block_id("max_cliques12"),
            ]
            .into_iter()
            .collect(),
            fitness: 12,
            is_blockclique: true,
        }],
        ledger: Default::default(),
    };

    let bytes = graph.to_bytes_compact().unwrap();
    let (new_graph, cursor) = BootstrapableGraph::from_bytes_compact(&bytes).unwrap();

    assert_eq!(bytes.len(), cursor);
    assert_eq!(
        graph.active_blocks[&b1_id].block.header.signature,
        new_graph.active_blocks[&b1_id].block.header.signature
    );
    assert_eq!(graph.best_parents[0], new_graph.best_parents[0]);
    assert_eq!(graph.best_parents[1], new_graph.best_parents[1]);
    assert_eq!(
        graph.latest_final_blocks_periods[0],
        new_graph.latest_final_blocks_periods[0]
    );
    assert_eq!(
        graph.latest_final_blocks_periods[1],
        new_graph.latest_final_blocks_periods[1]
    );
}

#[tokio::test]
#[serial]
async fn test_clique_calculation() {
    let ledger_file = generate_ledger_file(&Map::default());
    let cfg = ConsensusConfig::from(ledger_file.path());
    let mut block_graph = BlockGraph::new(GraphConfig::from(&cfg), None)
        .await
        .unwrap();
    let hashes: Vec<BlockId> = vec![
        "VzCRpnoZVYY1yQZTXtVQbbxwzdu6hYtdCUZB5BXWSabsiXyfP",
        "JnWwNHRR1tUD7UJfnEFgDB4S4gfDTX2ezLadr7pcwuZnxTvn1",
        "xtvLedxC7CigAPytS5qh9nbTuYyLbQKCfbX8finiHsKMWH6SG",
        "2Qs9sSbc5sGpVv5GnTeDkTKdDpKhp4AgCVT4XFcMaf55msdvJN",
        "2VNc8pR4tNnZpEPudJr97iNHxXbHiubNDmuaSzrxaBVwKXxV6w",
        "2bsrYpfLdvVWAJkwXoJz1kn4LWshdJ6QjwTrA7suKg8AY3ecH1",
        "kfUeGj3ZgBprqFRiAQpE47dW5tcKTAueVaWXZquJW6SaPBd4G",
    ]
    .into_iter()
    .map(|h| BlockId::from_bs58_check(h).unwrap())
    .collect();
    block_graph.gi_head = vec![
        (0, vec![1, 2, 3, 4]),
        (1, vec![0]),
        (2, vec![0]),
        (3, vec![0]),
        (4, vec![0]),
        (5, vec![6]),
        (6, vec![5]),
    ]
    .into_iter()
    .map(|(idx, lst)| (hashes[idx], lst.into_iter().map(|i| hashes[i]).collect()))
    .collect();
    let computed_sets = block_graph.compute_max_cliques();

    let expected_sets: Vec<Set<BlockId>> = vec![
        vec![1, 2, 3, 4, 5],
        vec![1, 2, 3, 4, 6],
        vec![0, 5],
        vec![0, 6],
    ]
    .into_iter()
    .map(|lst| lst.into_iter().map(|i| hashes[i]).collect())
    .collect();

    assert_eq!(computed_sets.len(), expected_sets.len());
    for expected in expected_sets.into_iter() {
        assert!(computed_sets.iter().any(|v| v == &expected));
    }
}

/// generate a named temporary JSON ledger file
fn generate_ledger_file(ledger_vec: &Map<Address, LedgerData>) -> NamedTempFile {
    use std::io::prelude::*;
    let ledger_file_named = NamedTempFile::new().expect("cannot create temp file");
    serde_json::to_writer_pretty(ledger_file_named.as_file(), &ledger_vec)
        .expect("unable to write ledger file");
    ledger_file_named
        .as_file()
        .seek(std::io::SeekFrom::Start(0))
        .expect("could not seek file");
    ledger_file_named
}
