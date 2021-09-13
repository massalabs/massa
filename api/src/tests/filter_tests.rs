// Copyright (c) 2021 MASSA LABS <info@massa.net>

use super::tools::*;
use crate::ApiEvent;
use crate::OperationIds;
use communication::network::Peer;
use communication::network::PeerInfo;
use communication::network::Peers;
use consensus::ExportBlockStatus;
use consensus::{DiscardReason, ExportCompiledBlock, Status};
use crypto::hash::Hash;
use models::address::{AddressState, Addresses};
use models::clique::ExportClique;
use models::ledger::LedgerData;
use models::SerializeCompact;
use models::StakersCycleProductionStats;
use models::{Address, Amount, Block, BlockHeader, BlockId, Slot};
use models::{
    Operation, OperationContent, OperationId, OperationSearchResult, OperationSearchResultStatus,
    OperationType,
};
use serde_json::json;
use serial_test::serial;
use std::str::FromStr;
use std::{
    collections::{HashMap, HashSet},
    net::{IpAddr, Ipv4Addr},
};
use storage::{start_storage, StorageConfig};
use time::UTime;

pub fn create_operation() -> Operation {
    let sender_priv = crypto::generate_random_private_key();
    let sender_pub = crypto::derive_public_key(&sender_priv);

    let recv_priv = crypto::generate_random_private_key();
    let recv_pub = crypto::derive_public_key(&recv_priv);

    let op = OperationType::Transaction {
        recipient_address: Address::from_public_key(&recv_pub).unwrap(),
        amount: Amount::default(),
    };
    let content = OperationContent {
        fee: Amount::default(),
        op,
        sender_public_key: sender_pub,
        expire_period: 0,
    };
    let hash = Hash::hash(&content.to_bytes_compact().unwrap());
    let signature = crypto::sign(&hash, &sender_priv).unwrap();

    Operation { content, signature }
}

#[tokio::test]
#[serial]
async fn test_get_operations() {
    initialize_context();

    //test no operation found
    {
        let (filter, mut rx_api) = mock_filter(None);

        // no input operation ids
        let res = warp::test::request()
            .method("GET")
            .path(&"/api/v1/get_operations")
            .matches(&filter)
            .await;
        assert!(!res);

        let handle = tokio::spawn(async move {
            let evt = rx_api.recv().await;
            match evt {
                Some(ApiEvent::GetOperations { response_tx, .. }) => {
                    response_tx
                        .send(HashMap::new())
                        .expect("failed to send block");
                }

                _ => {}
            }
        });

        let search_op_ids = OperationIds {
            operation_ids: vec![].into_iter().collect(),
        };

        //no provided op id.
        let url = format!(
            "/api/v1/get_operations?{}",
            serde_qs::to_string(&search_op_ids).unwrap(),
        );
        let res = warp::test::request()
            .method("GET")
            .path(&url)
            .reply(&filter)
            .await;
        assert_eq!(res.status(), 500);

        //op id provided but no op found.
        let search_op_ids = OperationIds {
            operation_ids: vec![OperationId::from_bs58_check(
                "SGoTK5TJ9ZcCgQVmdfma88UdhS6GK94aFEYAsU3F1inFayQ6S",
            )
            .unwrap()]
            .into_iter()
            .collect(),
        };
        let url = format!(
            "/api/v1/get_operations?{}",
            serde_qs::to_string(&search_op_ids).unwrap(),
        );
        let res = warp::test::request()
            .method("GET")
            .path(&url)
            .reply(&filter)
            .await;
        assert_eq!(res.status(), 200);
        let obtained: serde_json::Value = serde_json::from_slice(res.body()).unwrap();
        assert_eq!(obtained, serde_json::Value::Array(Vec::new()));
        handle.await.unwrap();
    }

    //test one operation found for several addresses
    {
        let (filter, mut rx_api) = mock_filter(None);

        let operation = create_operation();
        let operation_id = operation.get_operation_id().unwrap();
        let op_response = OperationSearchResult {
            status: OperationSearchResultStatus::Pending,
            op: operation,
            in_pool: false,
            in_blocks: vec![(BlockId(Hash::hash("test".as_bytes())), (23, true))]
                .into_iter()
                .collect(), // index, is_final
        };
        let handle_response = op_response.clone();
        let handle = tokio::spawn(async move {
            let evt = rx_api.recv().await;
            match evt {
                Some(ApiEvent::GetOperations { response_tx, .. }) => {
                    response_tx
                        .send(vec![(operation_id, handle_response)].into_iter().collect())
                        .expect("failed to send block");
                }

                _ => {}
            }
        });

        let search_op_ids = OperationIds {
            operation_ids: vec![
                operation_id,
                OperationId::from_bs58_check("SGoTK5TJ9ZcCgQVmdfma88UdhS6GK94aFEYAsU3F1inFayQ6S")
                    .unwrap(),
            ]
            .into_iter()
            .collect(),
        };

        let url = format!(
            "/api/v1/get_operations?{}",
            serde_qs::to_string(&search_op_ids).unwrap(),
        );
        let res = warp::test::request()
            .method("GET")
            .path(&url)
            .reply(&filter)
            .await;
        assert_eq!(res.status(), 200);

        let obtained: Vec<(OperationId, OperationSearchResult)> =
            serde_json::from_slice(res.body()).unwrap();

        assert_eq!(obtained.len(), 1);
        assert_eq!(obtained[0].0, operation_id);
        assert_eq!(
            obtained[0].1.op.get_operation_id().unwrap(),
            op_response.op.get_operation_id().unwrap()
        );
        assert_eq!(obtained[0].1.in_pool, op_response.in_pool);
        assert_eq!(obtained[0].1.in_blocks, op_response.in_blocks);
        handle.await.unwrap();
    }
}

#[tokio::test]
#[serial]
async fn test_get_addresses_info() {
    initialize_context();
    let _serialization_context = models::get_serialization_context();

    let search_address =
        Address::from_bs58_check("SGoTK5TJ9ZcCgQVmdfma88UdhS6GK94aFEYAsU3F1inFayQ6S").unwrap();

    //test no balance found
    {
        let (filter, mut rx_api) = mock_filter(None);

        // no input operation ids
        let res = warp::test::request()
            .method("GET")
            .path(&"/api/v1/addresses_info")
            .matches(&filter)
            .await;
        assert!(!res);

        let handle = tokio::spawn(async move {
            let evt = rx_api.recv().await;
            match evt {
                Some(ApiEvent::GetAddressesInfo { response_tx, .. }) => {
                    response_tx
                        .send(Ok(HashMap::new()))
                        .expect("failed to send data");
                }

                _ => {}
            }
        });

        let search_addrs = Addresses {
            addrs: vec![].into_iter().collect(),
        };

        //no provided address.
        let url = format!(
            "/api/v1/addresses_info?{}",
            serde_qs::to_string(&search_addrs).unwrap(),
        );
        let res = warp::test::request()
            .method("GET")
            .path(&url)
            .reply(&filter)
            .await;
        assert_eq!(res.status(), 500);

        //address provided but balance found.
        let search_addrs = Addresses {
            addrs: vec![search_address].into_iter().collect(),
        };
        let url = format!(
            "/api/v1/addresses_info?{}",
            serde_qs::to_string(&search_addrs).unwrap(),
        );
        let res = warp::test::request()
            .method("GET")
            .path(&url)
            .reply(&filter)
            .await;
        assert_eq!(res.status(), 200);
        let obtained: HashMap<Address, AddressState> = serde_json::from_slice(res.body()).unwrap();
        assert_eq!(obtained.len(), 0);

        handle.await.unwrap();
    }

    //test one operation found for several addresses
    {
        let mut addrs_info: HashMap<Address, AddressState> = HashMap::new();
        addrs_info.insert(
            search_address,
            AddressState {
                final_rolls: 1233,
                active_rolls: Some(5748),
                candidate_rolls: 7787,
                locked_balance: Amount::from_str("1745").unwrap(),
                candidate_ledger_data: LedgerData::new(Amount::from_str("4788").unwrap()),
                final_ledger_data: LedgerData::new(Amount::from_str("11414").unwrap()),
            },
        );
        let (filter, mut rx_api) = mock_filter(None);
        let cpy_addr_info = addrs_info.clone();
        let handle = tokio::spawn(async move {
            let evt = rx_api.recv().await;
            match evt {
                Some(ApiEvent::GetAddressesInfo { response_tx, .. }) => {
                    response_tx
                        .send(Ok(cpy_addr_info))
                        .expect("failed to send data");
                }

                _ => {}
            }
        });

        let search_addrs = Addresses {
            addrs: vec![
                search_address.clone(),
                Address::from_bs58_check("veCeaKsQbwMa7nnRsgZ5qTEyThWCMNi73w5Za9wdsdiEEkZLM")
                    .unwrap(),
            ]
            .into_iter()
            .collect(),
        };

        let url = format!(
            "/api/v1/addresses_info?{}",
            serde_qs::to_string(&search_addrs).unwrap(),
        );
        let res = warp::test::request()
            .method("GET")
            .path(&url)
            .reply(&filter)
            .await;
        assert_eq!(res.status(), 200);

        let obtained: HashMap<Address, AddressState> = serde_json::from_slice(res.body()).unwrap();

        assert_eq!(
            obtained[&search_address].final_rolls,
            addrs_info[&search_address].final_rolls
        );
        assert_eq!(
            obtained[&search_address].active_rolls,
            addrs_info[&search_address].active_rolls
        );
        assert_eq!(
            obtained[&search_address].candidate_rolls,
            addrs_info[&search_address].candidate_rolls
        );
        assert_eq!(
            obtained[&search_address].candidate_ledger_data.balance,
            addrs_info[&search_address].candidate_ledger_data.balance
        );
        assert_eq!(
            obtained[&search_address].final_ledger_data.balance,
            addrs_info[&search_address].final_ledger_data.balance
        );
        handle.await.unwrap();
    }
}

#[tokio::test]
#[serial]
async fn test_cliques() {
    {
        //test with no cliques
        let (filter, mut rx_api) = mock_filter(None);
        let handle = tokio::spawn(async move {
            let evt = rx_api.recv().await;

            match evt {
                Some(ApiEvent::GetBlockGraphStatus(response_sender_tx)) => {
                    response_sender_tx
                        .send(get_test_block_graph())
                        .expect("failed to send block graph");
                }

                _ => {}
            }
        });
        let res = warp::test::request()
            .method("GET")
            .path("/api/v1/cliques")
            .reply(&filter)
            .await;
        handle.await.unwrap();
        assert_eq!(res.status(), 200);
        let obtained: serde_json::Value = serde_json::from_slice(res.body()).unwrap();
        let expected: serde_json::Value = serde_json::from_str(
            &serde_json::to_string(&(0, Vec::<HashSet<BlockId>>::new())).unwrap(),
        )
        .unwrap();
        assert_eq!(obtained, expected);

        drop(filter);
    }

    //add default cliques
    let mut graph = get_test_block_graph();
    let hash_set = (0..1)
        .map(|_| get_test_block_id())
        .collect::<Vec<BlockId>>();
    graph.max_cliques = vec![
        ExportClique {
            block_ids: hash_set.clone(),
            is_blockclique: true,
            fitness: 2,
        },
        ExportClique {
            block_ids: hash_set.clone(),
            is_blockclique: false,
            fitness: 1,
        },
    ];
    let mut active_blocks = HashMap::new();
    active_blocks.insert(
        get_test_block_id(),
        ExportCompiledBlock {
            block: get_test_block().header,
            children: Vec::new(),
            status: consensus::Status::Active,
        },
    );
    graph.active_blocks = active_blocks;
    let cloned = graph.clone();

    let (filter, mut rx_api) = mock_filter(None);
    let handle = tokio::spawn(async move {
        let evt = rx_api.recv().await;

        match evt {
            Some(ApiEvent::GetBlockGraphStatus(response_sender_tx)) => {
                response_sender_tx
                    .send(cloned)
                    .expect("failed to send block graph");
            }

            _ => {}
        }
    });
    // invalid url parameter
    let matches = warp::test::request()
        .method("GET")
        .path(&"/api/v1/cliques/123")
        .matches(&filter)
        .await;
    assert!(!matches);

    //valid url with cliques.
    let res = warp::test::request()
        .method("GET")
        .path("/api/v1/cliques")
        .reply(&filter)
        .await;
    assert_eq!(res.status(), 200);
    handle.await.unwrap();
    let expected = hash_set
        .iter()
        .map(|hash| (hash, get_test_block().header.content.slot))
        .collect::<Vec<(&BlockId, Slot)>>();
    let obtained: serde_json::Value = serde_json::from_slice(res.body()).unwrap();
    let expected: serde_json::Value = serde_json::from_str(
        &serde_json::to_string(&(2, vec![expected.clone(), expected.clone()])).unwrap(),
    )
    .unwrap();
    assert_eq!(obtained, expected);

    drop(filter);
}

#[tokio::test]
#[serial]
async fn test_current_parents() {
    //test with empty parents
    {
        let (filter, mut rx_api) = mock_filter(None);
        let handle = tokio::spawn(async move {
            let evt = rx_api.recv().await;

            match evt {
                Some(ApiEvent::GetBlockGraphStatus(response_sender_tx)) => {
                    response_sender_tx
                        .send(get_test_block_graph())
                        .expect("failed to send block graph");
                }

                _ => {}
            }
        });
        let res = warp::test::request()
            .method("GET")
            .path("/api/v1/current_parents")
            .reply(&filter)
            .await;
        println!("{:?}", res);
        assert_eq!(res.status(), 200);
        let obtained: serde_json::Value = serde_json::from_slice(res.body()).unwrap();
        let expected: serde_json::Value =
            serde_json::from_str(&serde_json::to_string(&Vec::<BlockId>::new()).unwrap()).unwrap();
        assert_eq!(obtained, expected);
        handle.await.unwrap();

        drop(filter);
    }

    //add default parents
    let mut graph = get_test_block_graph();
    graph.best_parents = vec![(get_test_block_id(), 0), (get_test_block_id(), 0)];
    let mut active_blocks = HashMap::new();
    active_blocks.insert(
        get_test_block_id(),
        ExportCompiledBlock {
            block: get_test_block().header,
            children: Vec::new(),
            status: consensus::Status::Active,
        },
    );
    graph.active_blocks = active_blocks;
    let cloned = graph.clone();

    let (filter, mut rx_api) = mock_filter(None);
    let handle = tokio::spawn(async move {
        let evt = rx_api.recv().await;
        match evt {
            Some(ApiEvent::GetBlockGraphStatus(response_sender_tx)) => {
                response_sender_tx
                    .send(cloned)
                    .expect("failed to send block graph");
            }

            _ => {}
        }
    });
    // invalid url parameter
    let matches = warp::test::request()
        .method("GET")
        .path(&"/api/v1/current_parents/123")
        .matches(&filter)
        .await;
    println!("matches:{:?}", matches);
    assert!(!matches);

    //valid url with parents.
    let res = warp::test::request()
        .method("GET")
        .path("/api/v1/current_parents")
        .reply(&filter)
        .await;
    assert_eq!(res.status(), 200);

    handle.await.unwrap();
    let expected = (get_test_block_id(), get_test_block().header.content.slot);
    let obtained: serde_json::Value = serde_json::from_slice(res.body()).unwrap();
    let expected: serde_json::Value = serde_json::from_str(
        &serde_json::to_string(&vec![expected.clone(), expected.clone()]).unwrap(),
    )
    .unwrap();
    assert_eq!(obtained, expected);

    drop(filter);
}

#[tokio::test]
#[serial]
async fn test_get_graph_interval() {
    initialize_context();

    let (filter, mut rx_api) = mock_filter(None);

    let handle = tokio::spawn(async move {
        let evt = rx_api.recv().await;
        match evt {
            Some(ApiEvent::GetBlockGraphStatus(response_sender_tx)) => {
                response_sender_tx
                    .send(get_test_block_graph())
                    .expect("failed to send block graph");
            }

            _ => {}
        }
    });
    // invalid hash: filter mismatch
    let matches = warp::test::request()
        .method("GET")
        .path(&"/api/v1/graph_interval")
        .matches(&filter)
        .await;

    handle.await.unwrap();

    assert!(matches);
    let (filter, mut rx_api) = mock_filter(None);

    let handle = tokio::spawn(async move {
        let evt = rx_api.recv().await;
        match evt {
            Some(ApiEvent::GetBlockGraphStatus(response_sender_tx)) => {
                response_sender_tx
                    .send(get_test_block_graph())
                    .expect("failed to send block graph");
            }

            _ => {}
        }
    });

    // block not found
    let start: UTime = 0.into();
    let end: UTime = 0.into();
    let res = warp::test::request()
        .method("GET")
        .path(&format!(
            "/api/v1/graph_interval?start={}&end={}",
            start, end
        ))
        .reply(&filter)
        .await;
    assert_eq!(res.status(), 200);
    let obtained: serde_json::Value = serde_json::from_slice(res.body()).unwrap();
    let expected: serde_json::Value =
        serde_json::from_str(&serde_json::to_string(&Vec::<BlockId>::new()).unwrap()).unwrap();
    assert_eq!(obtained, expected);

    handle.await.expect("handle failed");

    let mut expected = get_test_block_graph();
    expected.active_blocks.insert(
        get_test_block_id(),
        get_test_compiled_exported_block(Slot::new(1, 0), None),
    );
    let cloned = expected.clone();

    let (filter, mut rx_api) = mock_filter(None);

    let handle = tokio::spawn(async move {
        let evt = rx_api.recv().await;
        match evt {
            Some(ApiEvent::GetBlockGraphStatus(response_sender_tx)) => {
                response_sender_tx
                    .send(cloned)
                    .expect("failed to send block graph");
            }

            _ => {}
        }
    });
    // block found
    let end: UTime = 2500.into();
    let res = warp::test::request()
        .method("GET")
        .path(&format!(
            "/api/v1/graph_interval?start={}&end={}",
            start, end
        ))
        .reply(&filter)
        .await;
    assert_eq!(res.status(), 200);
    handle.await.expect("handle failed");
    let obtained: serde_json::Value = serde_json::from_slice(res.body()).unwrap();
    let block = expected.active_blocks.get(&get_test_block_id()).unwrap();
    let expected = vec![(
        get_test_block_id(),
        block.block.content.slot,
        Status::Active,
        block.block.content.parents.clone(),
    )];
    let expected: serde_json::Value =
        serde_json::from_str(&serde_json::to_string(&expected).unwrap()).unwrap();
    assert_eq!(obtained, expected);

    drop(filter);

    // test link with storage
    let mut cfg = get_consensus_config();
    cfg.t0 = 2000.into();

    let (storage_command_tx, (block_a, block_b, block_c)) = get_test_storage(cfg).await;

    let (filter, rx_api) = mock_filter(Some(storage_command_tx.clone()));

    let handle = get_empty_graph_handle(rx_api);

    let start: UTime = 2500.into();
    let end: UTime = 9000.into();
    let res = warp::test::request()
        .method("GET")
        .path(&format!(
            "/api/v1/graph_interval?start={}&end={}",
            start, end
        ))
        .reply(&filter)
        .await;
    assert_eq!(res.status(), 200);
    let obtained: serde_json::Value = serde_json::from_slice(res.body()).unwrap();
    let obtained: Vec<(BlockId, Slot, Status, Vec<BlockId>)> =
        serde_json::from_value(obtained).unwrap();
    let mut expected = Vec::<(BlockId, Slot, Status, Vec<BlockId>)>::new();
    expected.push((
        block_b.header.compute_block_id().unwrap(),
        block_b.header.content.slot,
        Status::Final,
        block_b.header.content.parents.clone(),
    ));
    expected.push((
        block_c.header.compute_block_id().unwrap(),
        block_c.header.content.slot,
        Status::Final,
        block_c.header.content.parents.clone(),
    ));

    for item in obtained.iter() {
        assert!(expected.contains(&item))
    }
    for item in expected {
        assert!(obtained.contains(&item))
    }
    handle.await.unwrap();

    let (filter, rx_api) = mock_filter(Some(storage_command_tx.clone()));

    let handle = get_empty_graph_handle(rx_api);

    let start: UTime = 50.into();
    let end: UTime = 4500.into();
    let res = warp::test::request()
        .method("GET")
        .path(&format!(
            "/api/v1/graph_interval?start={}&end={}",
            start, end
        ))
        .reply(&filter)
        .await;
    assert_eq!(res.status(), 200);
    let obtained: serde_json::Value = serde_json::from_slice(res.body()).unwrap();
    let obtained: Vec<(BlockId, Slot, Status, Vec<BlockId>)> =
        serde_json::from_value(obtained).unwrap();
    let mut expected = Vec::<(BlockId, Slot, Status, Vec<BlockId>)>::new();
    expected.push((
        block_a.header.compute_block_id().unwrap(),
        block_a.header.content.slot,
        Status::Final,
        block_a.header.content.parents.clone(),
    ));
    expected.push((
        block_b.header.compute_block_id().unwrap(),
        block_b.header.content.slot,
        Status::Final,
        block_b.header.content.parents.clone(),
    ));
    expected.push((
        block_c.header.compute_block_id().unwrap(),
        block_c.header.content.slot,
        Status::Final,
        block_c.header.content.parents.clone(),
    ));
    for item in obtained.iter() {
        assert!(expected.contains(&item))
    }
    for item in expected {
        assert!(obtained.contains(&item))
    }
    handle.await.unwrap();

    let (filter, rx_api) = mock_filter(Some(storage_command_tx.clone()));

    let handle = get_empty_graph_handle(rx_api);

    let start: UTime = 50.into();
    let end: UTime = 4500.into();
    let res = warp::test::request()
        .method("GET")
        .path(&format!(
            "/api/v1/graph_interval?start={}&end={}",
            start, end
        ))
        .reply(&filter)
        .await;
    assert_eq!(res.status(), 200);
    let obtained: serde_json::Value = serde_json::from_slice(res.body()).unwrap();
    let obtained: Vec<(BlockId, Slot, Status, Vec<BlockId>)> =
        serde_json::from_value(obtained).unwrap();
    let mut expected = Vec::<(BlockId, Slot, Status, Vec<BlockId>)>::new();
    expected.push((
        block_a.header.compute_block_id().unwrap(),
        block_a.header.content.slot,
        Status::Final,
        block_a.header.content.parents.clone(),
    ));
    expected.push((
        block_b.header.compute_block_id().unwrap(),
        block_b.header.content.slot,
        Status::Final,
        block_b.header.content.parents.clone(),
    ));
    expected.push((
        block_c.header.compute_block_id().unwrap(),
        block_c.header.content.slot,
        Status::Final,
        block_c.header.content.parents.clone(),
    ));
    for item in obtained.iter() {
        assert!(expected.contains(&item))
    }
    for item in expected {
        assert!(obtained.contains(&item))
    }
    handle.await.unwrap();

    let (filter, rx_api) = mock_filter(Some(storage_command_tx.clone()));

    let handle = get_empty_graph_handle(rx_api);

    let start: UTime = 2500.into();
    let end: UTime = 3500.into();
    let res = warp::test::request()
        .method("GET")
        .path(&format!(
            "/api/v1/graph_interval?start={}&end={}",
            start, end
        ))
        .reply(&filter)
        .await;
    assert_eq!(res.status(), 200);
    let obtained: serde_json::Value = serde_json::from_slice(res.body()).unwrap();
    let obtained: Vec<(BlockId, Slot, Status, Vec<BlockId>)> =
        serde_json::from_value(obtained).unwrap();
    let mut expected = Vec::<(BlockId, Slot, Status, Vec<BlockId>)>::new();

    expected.push((
        block_b.header.compute_block_id().unwrap(),
        block_b.header.content.slot,
        Status::Final,
        block_b.header.content.parents.clone(),
    ));

    assert_eq!(obtained, expected);
    handle.await.unwrap();

    let (filter, rx_api) = mock_filter(Some(storage_command_tx.clone()));

    let handle = get_empty_graph_handle(rx_api);

    let start: UTime = 3500.into();
    let end: UTime = 4500.into();
    let res = warp::test::request()
        .method("GET")
        .path(&format!(
            "/api/v1/graph_interval?start={}&end={}",
            start, end
        ))
        .reply(&filter)
        .await;
    assert_eq!(res.status(), 200);
    let obtained: serde_json::Value = serde_json::from_slice(res.body()).unwrap();
    let obtained: Vec<(BlockId, Slot, Status, Vec<BlockId>)> =
        serde_json::from_value(obtained).unwrap();
    let mut expected = Vec::<(BlockId, Slot, Status, Vec<BlockId>)>::new();

    expected.push((
        block_c.header.compute_block_id().unwrap(),
        block_c.header.content.slot,
        Status::Final,
        block_c.header.content.parents.clone(),
    ));

    assert_eq!(obtained, expected);
    handle.await.unwrap();

    let (filter, rx_api) = mock_filter(Some(storage_command_tx.clone()));

    let handle = get_empty_graph_handle(rx_api);

    let start: UTime = 50.into();
    let end: UTime = 2000.into();
    let res = warp::test::request()
        .method("GET")
        .path(&format!(
            "/api/v1/graph_interval?start={}&end={}",
            start, end
        ))
        .reply(&filter)
        .await;
    assert_eq!(res.status(), 200);
    let obtained: serde_json::Value = serde_json::from_slice(res.body()).unwrap();
    let obtained: Vec<(BlockId, Slot, Status, Vec<BlockId>)> =
        serde_json::from_value(obtained).unwrap();
    let expected = Vec::<(BlockId, Slot, Status, Vec<BlockId>)>::new();
    assert_eq!(obtained, expected);
    handle.await.unwrap();
}

#[tokio::test]
#[serial]
async fn test_last_final() {
    //test with empty final block
    {
        let (filter, mut rx_api) = mock_filter(None);
        let handle = tokio::spawn(async move {
            let evt = rx_api.recv().await;
            match evt {
                Some(ApiEvent::GetBlockGraphStatus(response_sender_tx)) => {
                    response_sender_tx
                        .send(get_test_block_graph())
                        .expect("failed to send block graph");
                }

                _ => {}
            }
        });

        let res = warp::test::request()
            .method("GET")
            .path("/api/v1/last_final")
            .reply(&filter)
            .await;
        handle.await.expect("handle failed");
        assert_eq!(res.status(), 200);
        let obtained: serde_json::Value = serde_json::from_slice(res.body()).unwrap();
        let expected: serde_json::Value =
            serde_json::from_str(&serde_json::to_string(&Vec::<(BlockId, Slot)>::new()).unwrap())
                .unwrap();
        assert_eq!(obtained, expected);

        drop(filter);
    }

    let mut expected = get_test_block_graph();
    expected.latest_final_blocks_periods =
        vec![(get_test_block_id(), 10), (get_test_block_id(), 21)];
    let cloned_expected = expected.clone();

    let (filter, mut rx_api) = mock_filter(None);
    let handle = tokio::spawn(async move {
        let evt = rx_api.recv().await;
        match evt {
            Some(ApiEvent::GetBlockGraphStatus(response_sender_tx)) => {
                response_sender_tx
                    .send(cloned_expected)
                    .expect("failed to send block graph");
            }

            _ => {}
        }
    });
    // invalid url parameter
    let matches = warp::test::request()
        .method("GET")
        .path(&"/api/v1/last_final/123")
        .matches(&filter)
        .await;
    println!("matches:{:?}", matches);
    assert!(!matches);

    //valid url with final block.
    let res = warp::test::request()
        .method("GET")
        .path("/api/v1/last_final")
        .reply(&filter)
        .await;
    assert_eq!(res.status(), 200);
    let obtained: serde_json::Value = serde_json::from_slice(res.body()).unwrap();
    let expected: serde_json::Value = serde_json::from_str(
        &serde_json::to_string(
            &expected
                .latest_final_blocks_periods
                .iter()
                .enumerate()
                .map(|(thread_number, (hash, period))| {
                    (hash.clone(), Slot::new(*period, thread_number as u8))
                })
                .collect::<Vec<(BlockId, Slot)>>(),
        )
        .unwrap(),
    )
    .unwrap();
    assert_eq!(obtained, expected);
    handle.await.expect("handle failed");
    drop(filter);
}

#[tokio::test]
#[serial]
async fn test_peers() {
    let private_key = crypto::signature::generate_random_private_key();
    let node_id = communication::NodeId(crypto::signature::derive_public_key(&private_key));
    //test with empty final peers
    {
        let (filter, mut rx_api) = mock_filter(None);
        let handle = tokio::spawn(async move {
            let evt = rx_api.recv().await;
            match evt {
                Some(ApiEvent::GetPeers(response_sender_tx)) => {
                    response_sender_tx
                        .send(Peers {
                            our_node_id: node_id,
                            peers: HashMap::new(),
                        })
                        .expect("failed to send peers");
                }

                _ => {}
            }
        });
        let res = warp::test::request()
            .method("GET")
            .path("/api/v1/peers")
            .reply(&filter)
            .await;
        handle.await.unwrap();
        assert_eq!(res.status(), 200);
        let obtained: serde_json::Value = serde_json::from_slice(res.body()).unwrap();
        let expected: serde_json::Value = serde_json::from_str(
            &serde_json::to_string(&Peers {
                our_node_id: node_id,
                peers: HashMap::<IpAddr, Peer>::new(),
            })
            .unwrap(),
        )
        .unwrap();
        assert_eq!(obtained, expected);

        drop(filter);
    }

    //add default peers

    let peers = (0..2)
        .map(|index| {
            (
                IpAddr::V4(Ipv4Addr::new(169, 202, 0, index)),
                Peer {
                    peer_info: PeerInfo {
                        ip: IpAddr::V4(Ipv4Addr::new(169, 202, 0, index)),
                        banned: false,
                        bootstrap: false,
                        last_alive: None,
                        last_failure: None,
                        advertised: true,
                        active_out_connection_attempts: 1,
                        active_out_connections: 1,
                        active_in_connections: 1,
                    },
                    active_nodes: vec![],
                },
            )
        })
        .collect::<HashMap<IpAddr, Peer>>();
    let cloned = peers.clone();

    let (filter, mut rx_api) = mock_filter(None);
    let handle = tokio::spawn(async move {
        let evt = rx_api.recv().await;
        match evt {
            Some(ApiEvent::GetPeers(response_sender_tx)) => {
                response_sender_tx
                    .send(Peers {
                        our_node_id: node_id,
                        peers: cloned,
                    })
                    .expect("failed to send peers");
            }

            _ => {}
        }
    });

    // invalid url parameter
    let matches = warp::test::request()
        .method("GET")
        .path(&"/api/v1/peers/123")
        .matches(&filter)
        .await;
    assert!(!matches);

    //valid url with peers.
    let res = warp::test::request()
        .method("GET")
        .path("/api/v1/peers")
        .reply(&filter)
        .await;
    handle.await.unwrap();
    assert_eq!(res.status(), 200);
    let obtained: serde_json::Value = serde_json::from_slice(res.body()).unwrap();
    let expected: serde_json::Value = serde_json::from_str(
        &serde_json::to_string(&Peers {
            peers,
            our_node_id: node_id,
        })
        .unwrap(),
    )
    .unwrap();
    assert_eq!(obtained, expected);

    drop(filter);
}

#[tokio::test]
#[serial]
async fn test_get_block_interval() {
    initialize_context();

    let mut graph = get_test_block_graph();
    graph.best_parents = vec![(get_test_block_id(), 0), (get_test_block_id(), 0)];

    let mut active_blocks = HashMap::new();
    active_blocks.insert(
        get_test_block_id(),
        ExportCompiledBlock {
            block: get_test_block().header,
            children: Vec::new(),
            status: consensus::Status::Active,
        },
    );
    graph.active_blocks = active_blocks;

    let (filter, mut rx_api) = mock_filter(None);

    let handle = tokio::spawn(async move {
        let evt = rx_api.recv().await;
        match evt {
            Some(ApiEvent::GetBlockGraphStatus(response_sender_tx)) => {
                response_sender_tx
                    .send(get_test_block_graph())
                    .expect("failed to send block graph");
            }

            _ => {}
        }
    });
    // invalid hash: filter mismatch
    let matches = warp::test::request()
        .method("GET")
        .path(&"/api/v1/blockinterval")
        .matches(&filter)
        .await;
    assert!(matches);

    let (filter, _rx_api) = mock_filter(None);

    // block not found
    let start: UTime = 0.into();
    let end: UTime = 0.into();
    let res = warp::test::request()
        .method("GET")
        .path(&format!(
            "/api/v1/blockinterval?start={}&end={}",
            start, end
        ))
        .reply(&filter)
        .await;
    assert_eq!(res.status(), 200);
    let obtained: serde_json::Value = serde_json::from_slice(res.body()).unwrap();
    let expected: serde_json::Value =
        serde_json::from_str(&serde_json::to_string(&Vec::<(BlockId, Slot)>::new()).unwrap())
            .unwrap();
    assert_eq!(obtained, expected);
    handle.await.unwrap();

    let (filter, mut rx_api) = mock_filter(None);

    let handle = tokio::spawn(async move {
        let evt = rx_api.recv().await;
        match evt {
            Some(ApiEvent::GetBlockGraphStatus(response_sender_tx)) => {
                response_sender_tx
                    .send(graph)
                    .expect("failed to send block graph");
            }

            _ => {}
        }
    });
    // block found
    let end: UTime = 2500.into();
    let res = warp::test::request()
        .method("GET")
        .path(&format!(
            "/api/v1/blockinterval?start={}&end={}",
            start, end
        ))
        .reply(&filter)
        .await;
    assert_eq!(res.status(), 200);
    let obtained: serde_json::Value = serde_json::from_slice(res.body()).unwrap();
    let mut expected = Vec::new();
    expected.push((get_test_block_id(), get_test_block().header.content.slot));
    let expected: serde_json::Value =
        serde_json::from_str(&serde_json::to_string(&expected).unwrap()).unwrap();
    assert_eq!(obtained, expected);

    drop(filter);
    handle.await.unwrap();

    // test link with storage

    let mut cfg = get_consensus_config();
    cfg.t0 = 2000.into();

    let (storage_command_tx, (block_a, block_b, block_c)) = get_test_storage(cfg).await;

    let (filter, rx_api) = mock_filter(Some(storage_command_tx.clone()));

    let handle = get_empty_graph_handle(rx_api);

    let start: UTime = 50.into();
    let end: UTime = 2000.into();
    let res = warp::test::request()
        .method("GET")
        .path(&format!(
            "/api/v1/blockinterval?start={}&end={}",
            start, end
        ))
        .reply(&filter)
        .await;
    assert_eq!(res.status(), 200);
    let obtained: serde_json::Value = serde_json::from_slice(res.body()).unwrap();
    let expected = Vec::<(BlockId, Slot)>::new();
    let expected: serde_json::Value =
        serde_json::from_str(&serde_json::to_string(&expected).unwrap()).unwrap();
    assert_eq!(obtained, expected);
    handle.await.unwrap();

    let (filter, rx_api) = mock_filter(Some(storage_command_tx.clone()));

    let handle = get_empty_graph_handle(rx_api);

    let start: UTime = 2500.into();
    let end: UTime = 3500.into();
    let res = warp::test::request()
        .method("GET")
        .path(&format!(
            "/api/v1/blockinterval?start={}&end={}",
            start, end
        ))
        .reply(&filter)
        .await;
    assert_eq!(res.status(), 200);
    let obtained: serde_json::Value = serde_json::from_slice(res.body()).unwrap();
    let mut expected = Vec::<(BlockId, Slot)>::new();
    expected.push((
        block_b.header.compute_block_id().unwrap(),
        block_b.header.content.slot,
    ));
    let expected: serde_json::Value =
        serde_json::from_str(&serde_json::to_string(&expected).unwrap()).unwrap();
    assert_eq!(obtained, expected);
    handle.await.unwrap();

    let (filter, rx_api) = mock_filter(Some(storage_command_tx.clone()));

    let handle = get_empty_graph_handle(rx_api);

    let start: UTime = 50.into();
    let end: UTime = 4500.into();
    let res = warp::test::request()
        .method("GET")
        .path(&format!(
            "/api/v1/blockinterval?start={}&end={}",
            start, end
        ))
        .reply(&filter)
        .await;
    assert_eq!(res.status(), 200);
    let obtained: serde_json::Value = serde_json::from_slice(res.body()).unwrap();
    let obtained: Vec<(BlockId, Slot)> = serde_json::from_value(obtained).unwrap();
    let mut expected = Vec::<(BlockId, Slot)>::new();
    expected.push((
        block_a.header.compute_block_id().unwrap(),
        block_a.header.content.slot,
    ));
    expected.push((
        block_b.header.compute_block_id().unwrap(),
        block_b.header.content.slot,
    ));
    expected.push((
        block_c.header.compute_block_id().unwrap(),
        block_c.header.content.slot,
    ));
    for item in obtained.iter() {
        assert!(expected.contains(&item))
    }
    for item in expected {
        assert!(obtained.contains(&item))
    }
    handle.await.unwrap();

    let (filter, rx_api) = mock_filter(Some(storage_command_tx.clone()));

    let handle = get_empty_graph_handle(rx_api);

    let start: UTime = 3500.into();
    let end: UTime = 4500.into();
    let res = warp::test::request()
        .method("GET")
        .path(&format!(
            "/api/v1/blockinterval?start={}&end={}",
            start, end
        ))
        .reply(&filter)
        .await;
    assert_eq!(res.status(), 200);
    let obtained: serde_json::Value = serde_json::from_slice(res.body()).unwrap();
    let obtained: Vec<(BlockId, Slot)> = serde_json::from_value(obtained).unwrap();
    let mut expected = Vec::<(BlockId, Slot)>::new();
    expected.push((
        block_c.header.compute_block_id().unwrap(),
        block_c.header.content.slot,
    ));

    for item in obtained.iter() {
        assert!(expected.contains(&item))
    }
    for item in expected {
        assert!(obtained.contains(&item))
    }
    handle.await.unwrap();

    let (filter, rx_api) = mock_filter(Some(storage_command_tx.clone()));

    let handle = get_empty_graph_handle(rx_api);

    let start: UTime = 2500.into();
    let end: UTime = 9000.into();
    let res = warp::test::request()
        .method("GET")
        .path(&format!(
            "/api/v1/blockinterval?start={}&end={}",
            start, end
        ))
        .reply(&filter)
        .await;
    assert_eq!(res.status(), 200);
    let obtained: serde_json::Value = serde_json::from_slice(res.body()).unwrap();
    let obtained: Vec<(BlockId, Slot)> = serde_json::from_value(obtained).unwrap();
    let mut expected = Vec::<(BlockId, Slot)>::new();
    expected.push((
        block_b.header.compute_block_id().unwrap(),
        block_b.header.content.slot,
    ));
    expected.push((
        block_c.header.compute_block_id().unwrap(),
        block_c.header.content.slot,
    ));

    for item in obtained.iter() {
        assert!(expected.contains(&item))
    }
    for item in expected {
        assert!(obtained.contains(&item))
    }
    handle.await.unwrap();
}

#[tokio::test]
#[serial]
async fn test_get_block() {
    initialize_context();

    let (filter, mut rx_api) = mock_filter(None);

    let handle = tokio::spawn(async move {
        let evt = rx_api.recv().await;
        match evt {
            Some(ApiEvent::GetBlockStatus { response_tx, .. }) => {
                response_tx.send(None).expect("failed to send block");
            }

            _ => {}
        }
    });

    // invalid hash: filter mismatch
    let matches = warp::test::request()
        .method("GET")
        .path(&"/api/v1/block/123")
        .matches(&filter)
        .await;
    assert!(!matches);

    // block not found
    let other_hash = get_dummy_block_id("something else");
    let res = warp::test::request()
        .method("GET")
        .path(&format!("/api/v1/block/{}", other_hash))
        .reply(&filter)
        .await;
    assert_eq!(res.status(), 404);
    handle.await.unwrap();

    let (filter, mut rx_api) = mock_filter(None);

    let handle = tokio::spawn(async move {
        let evt = rx_api.recv().await;
        match evt {
            Some(ApiEvent::GetBlockStatus { response_tx, .. }) => {
                response_tx
                    .send(Some(ExportBlockStatus::Active(get_test_block())))
                    .expect("failed to send block");
            }

            _ => {}
        }
    });
    // block found
    let res = warp::test::request()
        .method("GET")
        .path(&format!("/api/v1/block/{}", get_test_block_id()))
        .reply(&filter)
        .await;
    assert_eq!(res.status(), 200);
    let obtained: serde_json::Value = serde_json::from_slice(res.body()).unwrap();
    let expected: serde_json::Value = serde_json::from_str(
        &serde_json::to_string(&ExportBlockStatus::Active(get_test_block())).unwrap(),
    )
    .unwrap();
    assert_eq!(obtained, expected);
    handle.await.unwrap();
    drop(filter);

    let tempdir = tempfile::tempdir().expect("cannot create temp dir");

    let storage_config = StorageConfig {
        /// Max number of bytes we want to store
        max_stored_blocks: 5,
        /// path to db
        path: tempdir.path().to_path_buf(), // in target to be ignored by git and different file between test.
        cache_capacity: 256,    // little to force flush cache
        flush_interval: None,   // default
        reset_at_startup: true, // if there was something in storage, it is not the case anymore
    };
    let (storage_command_tx, _storage_manager) = start_storage(storage_config).unwrap();
    let (filter, mut rx_api) = mock_filter(None);

    let handle = tokio::spawn(async move {
        let evt = rx_api.recv().await;
        match evt {
            Some(ApiEvent::GetBlockStatus { response_tx, .. }) => {
                response_tx.send(None).expect("failed to send block");
            }

            _ => {}
        }
    });

    // block not found
    let other_hash = get_dummy_block_id("something else");

    storage_command_tx
        .add_block(other_hash, get_test_block())
        .await
        .unwrap();

    let res = warp::test::request()
        .method("GET")
        .path(&format!("/api/v1/block/{}", other_hash))
        .reply(&filter)
        .await;
    assert_eq!(res.status(), 404);
    handle.await.unwrap();
}

#[tokio::test]
#[serial]
async fn test_network_info() {
    let private_key = crypto::signature::generate_random_private_key();
    let node_id = communication::NodeId(crypto::signature::derive_public_key(&private_key));
    //test with empty peer list
    {
        let (filter, mut rx_api) = mock_filter(None);

        let handle = tokio::spawn(async move {
            let evt = rx_api.recv().await;
            match evt {
                Some(ApiEvent::GetPeers(response_sender_tx)) => {
                    response_sender_tx
                        .send(Peers {
                            our_node_id: node_id,
                            peers: HashMap::new(),
                        })
                        .expect("failed to send peers");
                }

                _ => {}
            }
        });
        let res = warp::test::request()
            .method("GET")
            .path("/api/v1/network_info")
            .reply(&filter)
            .await;
        assert_eq!(res.status(), 200);
        let obtained: serde_json::Value = serde_json::from_slice(res.body()).unwrap();
        let expected: serde_json::Value = json!({
            "node_id" : node_id,
            "our_ip": IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)),
            "peers": HashMap::<IpAddr, String>::new(),
        });
        assert_eq!(obtained, expected);
        handle.await.unwrap();

        drop(filter);
    }

    //add default peers

    let peers = (0..2)
        .map(|index| {
            (
                IpAddr::V4(Ipv4Addr::new(169, 202, 0, index)),
                Peer {
                    peer_info: PeerInfo {
                        ip: IpAddr::V4(Ipv4Addr::new(169, 202, 0, index)),
                        banned: false,
                        bootstrap: false,
                        last_alive: None,
                        last_failure: None,
                        advertised: true,
                        active_out_connection_attempts: 1,
                        active_out_connections: 1,
                        active_in_connections: 1,
                    },
                    active_nodes: vec![],
                },
            )
        })
        .collect::<HashMap<IpAddr, Peer>>();
    let cloned = peers.clone();
    let (filter, mut rx_api) = mock_filter(None);

    let handle = tokio::spawn(async move {
        let evt = rx_api.recv().await;
        match evt {
            Some(ApiEvent::GetPeers(response_sender_tx)) => {
                response_sender_tx
                    .send(Peers {
                        our_node_id: node_id,
                        peers: cloned,
                    })
                    .expect("failed to send peers");
            }

            _ => {}
        }
    });

    //valid url with peers.
    let res = warp::test::request()
        .method("GET")
        .path("/api/v1/network_info")
        .reply(&filter)
        .await;
    assert_eq!(res.status(), 200);
    let obtained: serde_json::Value = serde_json::from_slice(res.body()).unwrap();
    let expected: serde_json::Value = json!({
        "node_id" : node_id,
        "our_ip": IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)),
        "peers": peers,
    });
    assert_eq!(obtained, expected);
    handle.await.unwrap();
    drop(filter);
}

#[tokio::test]
#[serial]
async fn test_state() {
    let private_key = crypto::signature::generate_random_private_key();
    let node_id = communication::NodeId(crypto::signature::derive_public_key(&private_key));
    //test with empty final peers
    {
        let (filter, mut rx_api) = mock_filter(None);

        let handle = tokio::spawn(async move {
            loop {
                let evt = rx_api.recv().await;
                match evt {
                    Some(ApiEvent::GetPeers(response_sender_tx)) => {
                        response_sender_tx
                            .send(Peers {
                                our_node_id: node_id,
                                peers: HashMap::new(),
                            })
                            .expect("failed to send peers");
                    }
                    Some(ApiEvent::GetBlockGraphStatus(response_tx)) => {
                        response_tx
                            .send(get_test_block_graph())
                            .expect("failed to send graph");
                    }

                    None => break,
                    _ => {}
                }
            }
        });

        let res = warp::test::request()
            .method("GET")
            .path("/api/v1/state")
            .reply(&filter)
            .await;

        handle.abort();
        assert_eq!(res.status(), 200);

        if let Ok(serde_json::Value::Object(obtained_map)) = serde_json::from_slice(res.body()) {
            assert_eq!(obtained_map["last_final"], json! {[]});
            assert_eq!(obtained_map["nb_cliques"], json! {0});
            assert_eq!(obtained_map["nb_peers"], json! {0});
            assert_eq!(obtained_map["our_ip"], json! {"127.0.0.1"});
        } else {
            panic!("wrong root object type");
        }

        drop(filter);
    }

    //add default peers

    let peers = (0..2)
        .map(|index| {
            (
                IpAddr::V4(Ipv4Addr::new(169, 202, 0, index)),
                Peer {
                    peer_info: PeerInfo {
                        ip: IpAddr::V4(Ipv4Addr::new(169, 202, 0, index)),
                        banned: false,
                        bootstrap: false,
                        last_alive: None,
                        last_failure: None,
                        advertised: true,
                        active_out_connection_attempts: 1,
                        active_out_connections: 1,
                        active_in_connections: 1,
                    },
                    active_nodes: vec![],
                },
            )
        })
        .collect::<HashMap<IpAddr, Peer>>();
    let cloned = peers.clone();

    let (filter, mut rx_api) = mock_filter(None);

    let handle = tokio::spawn(async move {
        loop {
            let evt = rx_api.recv().await;
            match evt {
                Some(ApiEvent::GetPeers(response_sender_tx)) => {
                    response_sender_tx
                        .send(Peers {
                            our_node_id: node_id,
                            peers: cloned.clone(),
                        })
                        .expect("failed to send peers");
                }
                Some(ApiEvent::GetBlockGraphStatus(response_tx)) => {
                    response_tx
                        .send(get_test_block_graph())
                        .expect("failed to send graph");
                }

                None => break,
                _ => {}
            }
        }
    });

    let res = warp::test::request()
        .method("GET")
        .path("/api/v1/state")
        .reply(&filter)
        .await;
    assert_eq!(res.status(), 200);
    handle.abort();
    if let Ok(serde_json::Value::Object(obtained_map)) = serde_json::from_slice(res.body()) {
        assert_eq!(obtained_map["last_final"], json! {[]});
        assert_eq!(obtained_map["nb_cliques"], json! {0});
        assert_eq!(obtained_map["nb_peers"], json! {peers.len()});
        assert_eq!(obtained_map["our_ip"], json! {"127.0.0.1"});
    } else {
        panic!("wrong root object type");
    }

    drop(filter);
}

#[tokio::test]
#[serial]
async fn test_last_stale() {
    initialize_context();

    //test with empty stale block
    {
        let (filter, mut rx_api) = mock_filter(None);
        let handle = tokio::spawn(async move {
            let evt = rx_api.recv().await;
            match evt {
                Some(ApiEvent::GetBlockGraphStatus(response_sender_tx)) => {
                    response_sender_tx
                        .send(get_test_block_graph())
                        .expect("failed to send block graph");
                }

                _ => {}
            }
        });
        let res = warp::test::request()
            .method("GET")
            .path("/api/v1/last_stale")
            .reply(&filter)
            .await;
        handle.await.unwrap();
        assert_eq!(res.status(), 200);
        let obtained: serde_json::Value = serde_json::from_slice(res.body()).unwrap();
        let expected: serde_json::Value =
            serde_json::from_str(&serde_json::to_string(&Vec::<(BlockId, Slot)>::new()).unwrap())
                .unwrap();
        assert_eq!(obtained, expected);

        drop(filter);
    }

    //add default stale blocks
    let mut graph = get_test_block_graph();
    graph.discarded_blocks.map.extend(vec![
        (
            get_test_block_id(),
            (
                DiscardReason::Invalid("for test reason".to_string()),
                get_header(Slot::new(1, 1), None).1,
            ),
        ),
        (
            get_another_test_block_id(),
            (DiscardReason::Stale, get_header(Slot::new(2, 0), None).1),
        ),
    ]);
    let cloned = graph.clone();

    let (filter, mut rx_api) = mock_filter(None);
    let handle = tokio::spawn(async move {
        let evt = rx_api.recv().await;
        match evt {
            Some(ApiEvent::GetBlockGraphStatus(response_sender_tx)) => {
                response_sender_tx
                    .send(cloned)
                    .expect("failed to send block graph");
            }

            _ => {}
        }
    });

    // valid url with final block.
    let res = warp::test::request()
        .method("GET")
        .path("/api/v1/last_stale")
        .reply(&filter)
        .await;
    handle.await.unwrap();
    assert_eq!(res.status(), 200);
    let obtained: serde_json::Value = serde_json::from_slice(res.body()).unwrap();
    let expected: serde_json::Value = serde_json::from_str(
        &serde_json::to_string(&vec![(get_another_test_block_id(), Slot::new(2, 0))]).unwrap(),
    )
    .unwrap();
    assert_eq!(obtained, expected);

    drop(filter);
}

#[tokio::test]
#[serial]
async fn test_last_invalid() {
    initialize_context();

    //test with empty final block
    {
        let (filter, mut rx_api) = mock_filter(None);
        let handle = tokio::spawn(async move {
            let evt = rx_api.recv().await;
            match evt {
                Some(ApiEvent::GetBlockGraphStatus(response_sender_tx)) => {
                    response_sender_tx
                        .send(get_test_block_graph())
                        .expect("failed to send block graph");
                }

                _ => {}
            }
        });
        let res = warp::test::request()
            .method("GET")
            .path("/api/v1/last_invalid")
            .reply(&filter)
            .await;
        handle.await.unwrap();
        assert_eq!(res.status(), 200);
        let obtained: serde_json::Value = serde_json::from_slice(res.body()).unwrap();
        let expected: serde_json::Value =
            serde_json::from_str(&serde_json::to_string(&Vec::<(BlockId, Slot)>::new()).unwrap())
                .unwrap();
        assert_eq!(obtained, expected);

        drop(filter);
    }

    //add default stale blocks
    let mut graph = get_test_block_graph();
    graph.discarded_blocks.map.extend(vec![
        (
            get_test_block_id(),
            (
                DiscardReason::Invalid("for test reason".to_string()),
                get_header(Slot::new(1, 1), None).1,
            ),
        ),
        (
            get_another_test_block_id(),
            (DiscardReason::Stale, get_header(Slot::new(2, 0), None).1),
        ),
    ]);
    let cloned = graph.clone();

    let (filter, mut rx_api) = mock_filter(None);
    let handle = tokio::spawn(async move {
        let evt = rx_api.recv().await;
        match evt {
            Some(ApiEvent::GetBlockGraphStatus(response_sender_tx)) => {
                response_sender_tx
                    .send(cloned)
                    .expect("failed to send block graph");
            }

            _ => {}
        }
    });

    //valid url with final block.
    let res = warp::test::request()
        .method("GET")
        .path("/api/v1/last_invalid")
        .reply(&filter)
        .await;
    handle.await.unwrap();
    assert_eq!(res.status(), 200);
    let obtained: serde_json::Value = serde_json::from_slice(res.body()).unwrap();
    let expected = serde_json::to_value(&vec![(get_test_block_id(), Slot::new(1, 1))]).unwrap();
    assert_eq!(obtained, expected);

    drop(filter);
}

#[tokio::test]
#[serial]
async fn test_staker_info() {
    initialize_context();

    let staker = get_dummy_staker();
    let staker_addr = Address::from_public_key(&staker).unwrap();
    let cloned_staker = staker.clone();

    let stats = StakersCycleProductionStats {
        cycle: 0,
        is_final: true,
        ok_nok_counts: vec![(staker_addr, (5, 0))].into_iter().collect(),
    };
    let mut staker_stats: Vec<StakersCycleProductionStats> = Vec::with_capacity(1);
    staker_stats.insert(0, stats);

    //test with empty final block
    {
        let (filter, mut rx_api) = mock_filter(None);

        let handle = tokio::spawn(async move {
            loop {
                let evt = rx_api.recv().await;
                match evt {
                    Some(ApiEvent::GetSelectionDraw { response_tx, .. }) => {
                        response_tx
                            .send(Ok(vec![(
                                Slot::new(0, 0),
                                (
                                    Address::from_public_key(&cloned_staker).unwrap(),
                                    Vec::new(),
                                ),
                            )]))
                            .expect("failed to send selection draw");
                    }
                    Some(ApiEvent::GetBlockGraphStatus(response_tx)) => {
                        response_tx
                            .send(get_test_block_graph())
                            .expect("failed to send graph");
                    }
                    Some(ApiEvent::GetStakersProductionStats { response_tx, .. }) => {
                        response_tx
                            .send(staker_stats.clone())
                            .expect("failed to send stats");
                    }

                    None => break,
                    _ => {}
                }
            }
        });

        let res = warp::test::request()
            .method("GET")
            .path(&format!(
                "/api/v1/staker_info/{}",
                Address::from_public_key(&staker).unwrap().to_bs58_check()
            ))
            .reply(&filter)
            .await;
        handle.abort();
        assert_eq!(res.status(), 200);
        let obtained: serde_json::Value = serde_json::from_slice(res.body()).unwrap();
        let empty_vec = serde_json::to_value(&Vec::<Block>::new()).unwrap();
        assert_eq!(obtained["staker_active_blocks"], empty_vec);
        assert_eq!(obtained["staker_discarded_blocks"], empty_vec);
        assert_eq!(
            obtained["staker_next_draws"],
            serde_json::to_value(vec![Slot::new(0u64, 0u8)]).unwrap()
        );

        drop(filter);
    }
    //add default stale blocks
    let mut graph = get_test_block_graph();

    let staker_s_discarded = vec![(
        get_test_block_id(),
        (
            DiscardReason::Invalid("for test reason".to_string()),
            get_header(Slot::new(1, 1), Some(staker)).1,
        ),
    )];
    graph.discarded_blocks.map.extend(vec![
        staker_s_discarded[0].clone(),
        (
            get_another_test_block_id(),
            (DiscardReason::Stale, get_header(Slot::new(2, 0), None).1),
        ),
    ]);

    let staker_s_active = vec![(
        get_another_test_block_id(),
        get_test_compiled_exported_block(Slot::new(2, 1), Some(staker)),
    )];
    graph
        .active_blocks
        .insert(staker_s_active[0].0.clone(), staker_s_active[0].1.clone());

    let cloned_staker = staker.clone();
    let cloned_graph = graph.clone();
    let (filter, mut rx_api) = mock_filter(None);
    let stats = vec![StakersCycleProductionStats {
        cycle: 0,
        is_final: true,
        ok_nok_counts: vec![(staker_addr, (5, 0))].into_iter().collect(),
    }];
    let handle = tokio::spawn(async move {
        loop {
            let cloned = cloned_graph.clone();
            let evt = rx_api.recv().await;
            match evt {
                Some(ApiEvent::GetSelectionDraw { response_tx, .. }) => {
                    response_tx
                        .send(Ok(vec![(
                            Slot::new(0, 0),
                            (
                                Address::from_public_key(&cloned_staker).unwrap(),
                                Vec::new(),
                            ),
                        )]))
                        .expect("failed to send selection draw");
                }
                Some(ApiEvent::GetBlockGraphStatus(response_tx)) => {
                    response_tx.send(cloned).expect("failed to send graph");
                }
                Some(ApiEvent::GetStakersProductionStats { response_tx, .. }) => {
                    response_tx
                        .send(stats.clone())
                        .expect("failed to send stats");
                }
                None => break,
                _ => {}
            }
        }
    });

    //valid url with final block.
    let res = warp::test::request()
        .method("GET")
        .path(&format!(
            "/api/v1/staker_info/{}",
            Address::from_public_key(&staker).unwrap().to_bs58_check()
        ))
        .reply(&filter)
        .await;
    handle.abort();

    assert_eq!(res.status(), 200);
    let obtained: serde_json::Value = serde_json::from_slice(res.body()).unwrap();
    let expected_active: serde_json::Value = serde_json::from_str(
        &serde_json::to_string(
            &staker_s_active
                .iter()
                .map(|(hash, compiled_block)| (hash, compiled_block.block.clone()))
                .collect::<Vec<(&BlockId, BlockHeader)>>(),
        )
        .unwrap(),
    )
    .unwrap();
    let expected_discarded: serde_json::Value = serde_json::from_str(
        &serde_json::to_string(
            &staker_s_discarded
                .iter()
                .map(|(hash, (reason, header))| (hash, reason, header))
                .collect::<Vec<(&BlockId, &DiscardReason, &BlockHeader)>>(),
        )
        .unwrap(),
    )
    .unwrap();
    assert_eq!(obtained["staker_active_blocks"], expected_active);
    assert_eq!(obtained["staker_discarded_blocks"], expected_discarded);

    drop(filter);
}
