use crate::ApiEvent;
use storage::{start_storage_controller, StorageConfig};

use super::tools::*;
use crate::filters::hash_slot_vec_to_json;
use communication::network::PeerInfo;
use consensus::{DiscardReason, ExportCompiledBlock};
use crypto::hash::Hash;
use models::{Block, BlockHeader, SerializationContext, Slot};
use serde_json::json;
use std::{
    collections::{HashMap, HashSet},
    net::{IpAddr, Ipv4Addr},
};
use time::UTime;

#[tokio::test]
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
            &serde_json::to_string(&(0, Vec::<HashSet<Hash>>::new())).unwrap(),
        )
        .unwrap();
        assert_eq!(obtained, expected);

        drop(filter);
    }

    //add default cliques
    let mut graph = get_test_block_graph();
    let hash_set = (0..2).map(|_| get_test_hash()).collect::<HashSet<Hash>>();
    graph.max_cliques = vec![hash_set.clone(), hash_set.clone()];
    let mut active_blocks = HashMap::new();
    active_blocks.insert(
        get_test_hash(),
        ExportCompiledBlock {
            block: get_test_block().header,
            children: Vec::new(),
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
    println!("matches:{:?}", matches);
    assert!(!matches);

    //valide url with cliques.
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
        .collect::<Vec<(&Hash, Slot)>>();
    let obtained: serde_json::Value = serde_json::from_slice(res.body()).unwrap();
    let expected: serde_json::Value = serde_json::from_str(
        &serde_json::to_string(&(2, vec![expected.clone(), expected.clone()])).unwrap(),
    )
    .unwrap();
    assert_eq!(obtained, expected);

    drop(filter);
}

#[tokio::test]
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
            serde_json::from_str(&serde_json::to_string(&Vec::<Hash>::new()).unwrap()).unwrap();
        assert_eq!(obtained, expected);
        handle.await.unwrap();

        drop(filter);
    }

    //add default parents
    let mut graph = get_test_block_graph();
    graph.best_parents = vec![get_test_hash(), get_test_hash()];
    let mut active_blocks = HashMap::new();
    active_blocks.insert(
        get_test_hash(),
        ExportCompiledBlock {
            block: get_test_block().header,
            children: Vec::new(),
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

    //valide url with parents.
    let res = warp::test::request()
        .method("GET")
        .path("/api/v1/current_parents")
        .reply(&filter)
        .await;
    assert_eq!(res.status(), 200);

    handle.await.unwrap();
    let expected = (get_test_hash(), get_test_block().header.content.slot);
    let obtained: serde_json::Value = serde_json::from_slice(res.body()).unwrap();
    let expected: serde_json::Value = serde_json::from_str(
        &serde_json::to_string(&hash_slot_vec_to_json(vec![
            expected.clone(),
            expected.clone(),
        ]))
        .unwrap(),
    )
    .unwrap();
    assert_eq!(obtained, expected);

    drop(filter);
}

#[tokio::test]
async fn test_get_graph_interval() {
    let serialization_context = SerializationContext {
        max_block_size: 1024 * 1024,
        max_block_operations: 1024,
        parent_count: 2,
        max_peer_list_length: 128,
        max_message_size: 3 * 1024 * 1024,
    };

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
        serde_json::from_str(&serde_json::to_string(&Vec::<Hash>::new()).unwrap()).unwrap();
    assert_eq!(obtained, expected);

    handle.await.expect("handle failed");

    let mut expected = get_test_block_graph();
    expected.active_blocks.insert(
        get_test_hash(),
        get_test_compiled_exported_block(&serialization_context, Slot::new(1, 0), None),
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
    let block = expected.active_blocks.get(&get_test_hash()).unwrap();
    let expected = vec![(
        get_test_hash(),
        block.block.content.slot,
        "active", // in tests there are no blocks in gi_head, so no just active blocks
        block.block.content.parents.clone(),
    )];
    let expected: serde_json::Value =
        serde_json::from_str(&serde_json::to_string(&expected).unwrap()).unwrap();
    assert_eq!(obtained, expected);

    drop(filter);

    // test link with storage
    let mut cfg = get_consensus_config();
    cfg.t0 = 2000.into();

    let (storage_command_tx, (block_a, block_b, block_c)) =
        get_test_storage(cfg, serialization_context.clone()).await;

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
    let obtained: Vec<(Hash, Slot, String, Vec<Hash>)> = serde_json::from_value(obtained).unwrap();
    let mut expected = Vec::<(Hash, Slot, String, Vec<Hash>)>::new();
    expected.push((
        block_b
            .header
            .content
            .compute_hash(&serialization_context)
            .unwrap(),
        block_b.header.content.slot,
        "final".to_string(),
        Vec::new(),
    ));
    expected.push((
        block_c
            .header
            .content
            .compute_hash(&serialization_context)
            .unwrap(),
        block_c.header.content.slot,
        "final".to_string(),
        Vec::new(),
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
    let obtained: Vec<(Hash, Slot, String, Vec<Hash>)> = serde_json::from_value(obtained).unwrap();
    let mut expected = Vec::<(Hash, Slot, String, Vec<Hash>)>::new();
    expected.push((
        block_a
            .header
            .content
            .compute_hash(&serialization_context)
            .unwrap(),
        block_a.header.content.slot,
        "final".to_string(),
        Vec::new(),
    ));
    expected.push((
        block_b
            .header
            .content
            .compute_hash(&serialization_context)
            .unwrap(),
        block_b.header.content.slot,
        "final".to_string(),
        Vec::new(),
    ));
    expected.push((
        block_c
            .header
            .content
            .compute_hash(&serialization_context)
            .unwrap(),
        block_c.header.content.slot,
        "final".to_string(),
        Vec::new(),
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
    let obtained: Vec<(Hash, Slot, String, Vec<Hash>)> = serde_json::from_value(obtained).unwrap();
    let mut expected = Vec::<(Hash, Slot, String, Vec<Hash>)>::new();
    expected.push((
        block_a
            .header
            .content
            .compute_hash(&serialization_context)
            .unwrap(),
        block_a.header.content.slot,
        "final".to_string(),
        Vec::new(),
    ));
    expected.push((
        block_b
            .header
            .content
            .compute_hash(&serialization_context)
            .unwrap(),
        block_b.header.content.slot,
        "final".to_string(),
        Vec::new(),
    ));
    expected.push((
        block_c
            .header
            .content
            .compute_hash(&serialization_context)
            .unwrap(),
        block_c.header.content.slot,
        "final".to_string(),
        Vec::new(),
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
    let obtained: Vec<(Hash, Slot, String, Vec<Hash>)> = serde_json::from_value(obtained).unwrap();
    let mut expected = Vec::<(Hash, Slot, String, Vec<Hash>)>::new();

    expected.push((
        block_b
            .header
            .content
            .compute_hash(&serialization_context)
            .unwrap(),
        block_b.header.content.slot,
        "final".to_string(),
        Vec::new(),
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
    let obtained: Vec<(Hash, Slot, String, Vec<Hash>)> = serde_json::from_value(obtained).unwrap();
    let mut expected = Vec::<(Hash, Slot, String, Vec<Hash>)>::new();

    expected.push((
        block_c
            .header
            .content
            .compute_hash(&serialization_context)
            .unwrap(),
        block_c.header.content.slot,
        "final".to_string(),
        Vec::new(),
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
    let obtained: Vec<(Hash, Slot, String, Vec<Hash>)> = serde_json::from_value(obtained).unwrap();
    let expected = Vec::<(Hash, Slot, String, Vec<Hash>)>::new();
    assert_eq!(obtained, expected);
    handle.await.unwrap();
}

#[tokio::test]
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
            serde_json::from_str(&serde_json::to_string(&Vec::<(Hash, Slot)>::new()).unwrap())
                .unwrap();
        assert_eq!(obtained, expected);

        drop(filter);
    }

    let mut expected = get_test_block_graph();
    expected.latest_final_blocks_periods = vec![(get_test_hash(), 10), (get_test_hash(), 21)];
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

    //valide url with final block.
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
                .collect::<Vec<(Hash, Slot)>>(),
        )
        .unwrap(),
    )
    .unwrap();
    assert_eq!(obtained, expected);
    handle.await.expect("handle failed");
    drop(filter);
}

#[tokio::test]
async fn test_peers() {
    //test with empty final peers
    {
        let (filter, mut rx_api) = mock_filter(None);
        let handle = tokio::spawn(async move {
            let evt = rx_api.recv().await;
            match evt {
                Some(ApiEvent::GetPeers(response_sender_tx)) => {
                    response_sender_tx
                        .send(HashMap::new())
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
            &serde_json::to_string(&HashMap::<IpAddr, String>::new()).unwrap(),
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
                PeerInfo {
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
            )
        })
        .collect::<HashMap<IpAddr, PeerInfo>>();
    let cloned = peers.clone();

    let (filter, mut rx_api) = mock_filter(None);
    let handle = tokio::spawn(async move {
        let evt = rx_api.recv().await;
        match evt {
            Some(ApiEvent::GetPeers(response_sender_tx)) => {
                response_sender_tx
                    .send(cloned)
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

    //valide url with peers.
    let res = warp::test::request()
        .method("GET")
        .path("/api/v1/peers")
        .reply(&filter)
        .await;
    handle.await.unwrap();
    assert_eq!(res.status(), 200);
    let obtained: serde_json::Value = serde_json::from_slice(res.body()).unwrap();
    let expected: serde_json::Value =
        serde_json::from_str(&serde_json::to_string(&peers).unwrap()).unwrap();
    assert_eq!(obtained, expected);

    drop(filter);
}

#[tokio::test]
async fn test_get_block_interval() {
    let serialization_context = SerializationContext {
        max_block_size: 1024 * 1024,
        max_block_operations: 1024,
        parent_count: 2,
        max_peer_list_length: 128,
        max_message_size: 3 * 1024 * 1024,
    };

    let mut graph = get_test_block_graph();
    graph.best_parents = vec![get_test_hash(), get_test_hash()];

    let mut active_blocks = HashMap::new();
    active_blocks.insert(
        get_test_hash(),
        ExportCompiledBlock {
            block: get_test_block().header,
            children: Vec::new(),
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
        serde_json::from_str(&serde_json::to_string(&Vec::<(Hash, Slot)>::new()).unwrap()).unwrap();
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
    expected.push((get_test_hash(), get_test_block().header.content.slot));
    let expected: serde_json::Value =
        serde_json::from_str(&serde_json::to_string(&expected).unwrap()).unwrap();
    assert_eq!(obtained, expected);

    drop(filter);
    handle.await.unwrap();

    // test link with storage

    let mut cfg = get_consensus_config();
    cfg.t0 = 2000.into();

    let (storage_command_tx, (block_a, block_b, block_c)) =
        get_test_storage(cfg, serialization_context.clone()).await;

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
    let expected = Vec::<(Hash, Slot)>::new();
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
    let mut expected = Vec::<(Hash, Slot)>::new();
    expected.push((
        block_b
            .header
            .content
            .compute_hash(&serialization_context)
            .unwrap(),
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
    let obtained: Vec<(Hash, Slot)> = serde_json::from_value(obtained).unwrap();
    let mut expected = Vec::<(Hash, Slot)>::new();
    expected.push((
        block_a
            .header
            .content
            .compute_hash(&serialization_context)
            .unwrap(),
        block_a.header.content.slot,
    ));
    expected.push((
        block_b
            .header
            .content
            .compute_hash(&serialization_context)
            .unwrap(),
        block_b.header.content.slot,
    ));
    expected.push((
        block_c
            .header
            .content
            .compute_hash(&serialization_context)
            .unwrap(),
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
    let obtained: Vec<(Hash, Slot)> = serde_json::from_value(obtained).unwrap();
    let mut expected = Vec::<(Hash, Slot)>::new();
    expected.push((
        block_c
            .header
            .content
            .compute_hash(&serialization_context)
            .unwrap(),
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
    let obtained: Vec<(Hash, Slot)> = serde_json::from_value(obtained).unwrap();
    let mut expected = Vec::<(Hash, Slot)>::new();
    expected.push((
        block_b
            .header
            .content
            .compute_hash(&serialization_context)
            .unwrap(),
        block_b.header.content.slot,
    ));
    expected.push((
        block_c
            .header
            .content
            .compute_hash(&serialization_context)
            .unwrap(),
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
async fn test_get_block() {
    let serialization_context = SerializationContext {
        max_block_size: 1024 * 1024,
        max_block_operations: 1024,
        parent_count: 2,
        max_peer_list_length: 128,
        max_message_size: 3 * 1024 * 1024,
    };

    let (filter, mut rx_api) = mock_filter(None);

    let handle = tokio::spawn(async move {
        let evt = rx_api.recv().await;
        match evt {
            Some(ApiEvent::GetActiveBlock(_hash, response_sender_tx)) => {
                response_sender_tx.send(None).expect("failed to send block");
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
    let other_hash = Hash::hash("something else".as_bytes());
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
            Some(ApiEvent::GetActiveBlock(_hash, response_sender_tx)) => {
                response_sender_tx
                    .send(Some(get_test_block()))
                    .expect("failed to send block");
            }

            _ => {}
        }
    });
    // block found
    let res = warp::test::request()
        .method("GET")
        .path(&format!("/api/v1/block/{}", get_test_hash()))
        .reply(&filter)
        .await;
    assert_eq!(res.status(), 200);
    let obtained: serde_json::Value = serde_json::from_slice(res.body()).unwrap();
    let expected: serde_json::Value =
        serde_json::from_str(&serde_json::to_string(&get_test_block()).unwrap()).unwrap();
    assert_eq!(obtained, expected);
    handle.await.unwrap();
    drop(filter);

    let tempdir = tempfile::tempdir().expect("cannot create temp dir");

    let storage_config = StorageConfig {
        /// Max number of bytes we want to store
        max_stored_blocks: 5,
        /// path to db
        path: tempdir.path().to_path_buf(), //in target to be ignored by git and different file between test.
        cache_capacity: 256,  //little to force flush cache
        flush_interval: None, //defaut
    };
    let (storage_command_tx, _storage_manager) =
        start_storage_controller(storage_config, serialization_context.clone()).unwrap();
    let (filter, mut rx_api) = mock_filter(None);

    let handle = tokio::spawn(async move {
        let evt = rx_api.recv().await;
        match evt {
            Some(ApiEvent::GetActiveBlock(_hash, response_sender_tx)) => {
                response_sender_tx.send(None).expect("failed to send block");
            }

            _ => {}
        }
    });

    // block not found
    let other_hash = Hash::hash("something else".as_bytes());

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
async fn test_network_info() {
    //test with empty peer list
    {
        let (filter, mut rx_api) = mock_filter(None);

        let handle = tokio::spawn(async move {
            let evt = rx_api.recv().await;
            match evt {
                Some(ApiEvent::GetPeers(response_sender_tx)) => {
                    response_sender_tx
                        .send(HashMap::new())
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
                PeerInfo {
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
            )
        })
        .collect::<HashMap<IpAddr, PeerInfo>>();
    let cloned = peers.clone();
    let (filter, mut rx_api) = mock_filter(None);

    let handle = tokio::spawn(async move {
        let evt = rx_api.recv().await;
        match evt {
            Some(ApiEvent::GetPeers(response_sender_tx)) => {
                response_sender_tx
                    .send(cloned)
                    .expect("failed to send peers");
            }

            _ => {}
        }
    });

    //valide url with peers.
    let res = warp::test::request()
        .method("GET")
        .path("/api/v1/network_info")
        .reply(&filter)
        .await;
    assert_eq!(res.status(), 200);
    let obtained: serde_json::Value = serde_json::from_slice(res.body()).unwrap();
    let expected: serde_json::Value = json!({
        "our_ip": IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)),
        "peers": peers,
    });
    assert_eq!(obtained, expected);
    handle.await.unwrap();
    drop(filter);
}

#[tokio::test]
async fn test_state() {
    //test with empty final peers
    {
        let (filter, mut rx_api) = mock_filter(None);

        let handle = tokio::spawn(async move {
            loop {
                let evt = rx_api.recv().await;
                match evt {
                    Some(ApiEvent::GetPeers(response_sender_tx)) => {
                        response_sender_tx
                            .send(HashMap::new())
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
                PeerInfo {
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
            )
        })
        .collect::<HashMap<IpAddr, PeerInfo>>();
    let cloned = peers.clone();

    let (filter, mut rx_api) = mock_filter(None);

    let handle = tokio::spawn(async move {
        loop {
            let evt = rx_api.recv().await;
            match evt {
                Some(ApiEvent::GetPeers(response_sender_tx)) => {
                    response_sender_tx
                        .send(cloned.clone())
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
async fn test_last_stale() {
    let serialization_context = SerializationContext {
        max_block_size: 1024 * 1024,
        max_block_operations: 1024,
        parent_count: 2,
        max_peer_list_length: 128,
        max_message_size: 3 * 1024 * 1024,
    };

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
            serde_json::from_str(&serde_json::to_string(&Vec::<(Hash, Slot)>::new()).unwrap())
                .unwrap();
        assert_eq!(obtained, expected);

        drop(filter);
    }

    //add default stale blocks
    let mut graph = get_test_block_graph();
    graph.discarded_blocks.map.extend(vec![
        (
            get_test_hash(),
            (
                DiscardReason::Invalid,
                get_header(&serialization_context, Slot::new(1, 1), None).1,
            ),
        ),
        (
            get_another_test_hash(),
            (
                DiscardReason::Stale,
                get_header(&serialization_context, Slot::new(2, 0), None).1,
            ),
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
        &serde_json::to_string(&vec![(get_another_test_hash(), Slot::new(2, 0))]).unwrap(),
    )
    .unwrap();
    assert_eq!(obtained, expected);

    drop(filter);
}

#[tokio::test]
async fn test_last_invalid() {
    let serialization_context = SerializationContext {
        max_block_size: 1024 * 1024,
        max_block_operations: 1024,
        parent_count: 2,
        max_peer_list_length: 128,
        max_message_size: 3 * 1024 * 1024,
    };

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
            serde_json::from_str(&serde_json::to_string(&Vec::<(Hash, Slot)>::new()).unwrap())
                .unwrap();
        assert_eq!(obtained, expected);

        drop(filter);
    }

    //add default stale blocks
    let mut graph = get_test_block_graph();
    graph.discarded_blocks.map.extend(vec![
        (
            get_test_hash(),
            (
                DiscardReason::Invalid,
                get_header(&serialization_context, Slot::new(1, 1), None).1,
            ),
        ),
        (
            get_another_test_hash(),
            (
                DiscardReason::Stale,
                get_header(&serialization_context, Slot::new(2, 0), None).1,
            ),
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

    //valide url with final block.
    let res = warp::test::request()
        .method("GET")
        .path("/api/v1/last_invalid")
        .reply(&filter)
        .await;
    handle.await.unwrap();
    assert_eq!(res.status(), 200);
    let obtained: serde_json::Value = serde_json::from_slice(res.body()).unwrap();
    let expected = serde_json::to_value(&vec![(get_test_hash(), Slot::new(1, 1))]).unwrap();
    assert_eq!(obtained, expected);

    drop(filter);
}

#[tokio::test]
async fn test_staker_info() {
    let serialization_context = SerializationContext {
        max_block_size: 1024 * 1024,
        max_block_operations: 1024,
        parent_count: 2,
        max_peer_list_length: 128,
        max_message_size: 3 * 1024 * 1024,
    };

    let staker = get_dummy_staker();
    let cloned_staker = staker.clone();
    //test with empty final block
    {
        let (filter, mut rx_api) = mock_filter(None);

        let handle = tokio::spawn(async move {
            loop {
                let evt = rx_api.recv().await;
                match evt {
                    Some(ApiEvent::GetSelectionDraw(_start, _stop, response_sender_tx)) => {
                        response_sender_tx
                            .send(Ok(vec![(Slot::new(0, 0), cloned_staker)]))
                            .expect("failed to send slection draw");
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
            .path(&format!("/api/v1/staker_info/{}", staker))
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
        get_test_hash(),
        (
            DiscardReason::Invalid,
            get_header(&serialization_context, Slot::new(1, 1), Some(staker)).1,
        ),
    )];
    graph.discarded_blocks.map.extend(vec![
        staker_s_discarded[0].clone(),
        (
            get_another_test_hash(),
            (
                DiscardReason::Stale,
                get_header(&serialization_context, Slot::new(2, 0), None).1,
            ),
        ),
    ]);

    let staker_s_active = vec![(
        get_another_test_hash(),
        get_test_compiled_exported_block(&serialization_context, Slot::new(2, 1), Some(staker)),
    )];
    graph
        .active_blocks
        .insert(staker_s_active[0].0.clone(), staker_s_active[0].1.clone());

    let cloned_staker = staker.clone();
    let cloned_graph = graph.clone();
    let (filter, mut rx_api) = mock_filter(None);

    let handle = tokio::spawn(async move {
        loop {
            let cloned = cloned_graph.clone();
            let evt = rx_api.recv().await;
            match evt {
                Some(ApiEvent::GetSelectionDraw(_start, _stop, response_sender_tx)) => {
                    response_sender_tx
                        .send(Ok(vec![(Slot::new(0, 0), cloned_staker)]))
                        .expect("failed to send selection draw");
                }
                Some(ApiEvent::GetBlockGraphStatus(response_tx)) => {
                    response_tx.send(cloned).expect("failed to send graph");
                }

                None => break,
                _ => {}
            }
        }
    });

    //valide url with final block.
    let res = warp::test::request()
        .method("GET")
        .path(&format!("/api/v1/staker_info/{}", staker))
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
                .collect::<Vec<(&Hash, BlockHeader)>>(),
        )
        .unwrap(),
    )
    .unwrap();
    let expected_discarded: serde_json::Value = serde_json::from_str(
        &serde_json::to_string(
            &staker_s_discarded
                .iter()
                .map(|(hash, (reason, header))| (hash, reason, header))
                .collect::<Vec<(&Hash, &DiscardReason, &BlockHeader)>>(),
        )
        .unwrap(),
    )
    .unwrap();
    assert_eq!(obtained["staker_active_blocks"], expected_active);
    assert_eq!(obtained["staker_discarded_blocks"], expected_discarded);

    drop(filter);
}
