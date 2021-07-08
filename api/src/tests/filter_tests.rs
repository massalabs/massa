use super::{mock_consensus_cmd::*, mock_network_cmd::*, mock_protocol_cmd::*, tools::*};
use communication::network::PeerInfo;
use consensus::DiscardReason;
use crypto::hash::Hash;
use models::block::{Block, BlockHeader};
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
        let (consensus_join_handle, consensus_cmd) = start_mock_consensus(ConsensusMockData::new());
        let (protocol_join_handle, protocol_cmd) = start_mock_protocol();
        let (network_join_handle, network_cmd) = start_mock_network(NetworkMockData::new());

        let filter = mock_filter(
            consensus_cmd.clone(),
            protocol_cmd.clone(),
            network_cmd.clone(),
        );
        let res = warp::test::request()
            .method("GET")
            .path("/api/v1/cliques")
            .reply(&filter)
            .await;
        assert_eq!(res.status(), 200);
        let obtained: serde_json::Value = serde_json::from_slice(res.body()).unwrap();
        let expected: serde_json::Value = serde_json::from_str(
            &serde_json::to_string(&(0, Vec::<HashSet<Hash>>::new())).unwrap(),
        )
        .unwrap();
        assert_eq!(obtained, expected);

        drop(filter);
        drop(consensus_cmd);
        drop(protocol_cmd);
        drop(network_cmd);
        consensus_join_handle.await.unwrap();
        protocol_join_handle.await.unwrap();
        network_join_handle.await.unwrap();
    }

    //add default cliques
    let mut mock_consensus_data = ConsensusMockData::new();
    let hash_set = (0..2).map(|_| get_test_hash()).collect::<HashSet<Hash>>();
    mock_consensus_data.graph.max_cliques = vec![hash_set.clone(), hash_set.clone()];
    mock_consensus_data.add_active_blocks(get_test_hash(), get_test_block()); // to make graph consistent

    let (consensus_join_handle, consensus_cmd) = start_mock_consensus(mock_consensus_data);
    let (protocol_join_handle, protocol_cmd) = start_mock_protocol();
    let (network_join_handle, network_cmd) = start_mock_network(NetworkMockData::new());

    let filter = mock_filter(
        consensus_cmd.clone(),
        protocol_cmd.clone(),
        network_cmd.clone(),
    );
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
    let expected = hash_set
        .iter()
        .map(|hash| {
            (
                hash,
                (
                    get_test_block().header.period_number,
                    get_test_block().header.thread_number,
                ),
            )
        })
        .collect::<Vec<(&Hash, (u64, u8))>>();
    let obtained: serde_json::Value = serde_json::from_slice(res.body()).unwrap();
    let expected: serde_json::Value = serde_json::from_str(
        &serde_json::to_string(&(2, vec![expected.clone(), expected.clone()])).unwrap(),
    )
    .unwrap();
    assert_eq!(obtained, expected);

    drop(filter);
    drop(consensus_cmd);
    drop(protocol_cmd);
    drop(network_cmd);
    consensus_join_handle.await.unwrap();
    protocol_join_handle.await.unwrap();
    network_join_handle.await.unwrap();
}

#[tokio::test]
async fn test_current_parents() {
    //test with empty parents
    {
        //test with no cliques
        let (consensus_join_handle, consensus_cmd) = start_mock_consensus(ConsensusMockData::new());
        let (protocol_join_handle, protocol_cmd) = start_mock_protocol();
        let (network_join_handle, network_cmd) = start_mock_network(NetworkMockData::new());

        let filter = mock_filter(
            consensus_cmd.clone(),
            protocol_cmd.clone(),
            network_cmd.clone(),
        );
        let res = warp::test::request()
            .method("GET")
            .path("/api/v1/current_parents")
            .reply(&filter)
            .await;
        assert_eq!(res.status(), 200);
        let obtained: serde_json::Value = serde_json::from_slice(res.body()).unwrap();
        let expected: serde_json::Value =
            serde_json::from_str(&serde_json::to_string(&Vec::<Hash>::new()).unwrap()).unwrap();
        assert_eq!(obtained, expected);

        drop(filter);
        drop(consensus_cmd);
        drop(protocol_cmd);
        drop(network_cmd);
        consensus_join_handle.await.unwrap();
        protocol_join_handle.await.unwrap();
        network_join_handle.await.unwrap();
    }

    //add default parents
    let mut mock_consensus_data = ConsensusMockData::new();
    mock_consensus_data.graph.best_parents = vec![get_test_hash(), get_test_hash()];
    mock_consensus_data.add_active_blocks(get_test_hash(), get_test_block());

    let (consensus_join_handle, consensus_cmd) = start_mock_consensus(mock_consensus_data);
    let (protocol_join_handle, protocol_cmd) = start_mock_protocol();
    let (network_join_handle, network_cmd) = start_mock_network(NetworkMockData::new());

    let filter = mock_filter(
        consensus_cmd.clone(),
        protocol_cmd.clone(),
        network_cmd.clone(),
    );
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

    let expected = (
        get_test_hash(),
        (
            get_test_block().header.period_number,
            get_test_block().header.thread_number,
        ),
    );
    let obtained: serde_json::Value = serde_json::from_slice(res.body()).unwrap();
    let expected: serde_json::Value = serde_json::from_str(
        &serde_json::to_string(&vec![expected.clone(), expected.clone()]).unwrap(),
    )
    .unwrap();
    assert_eq!(obtained, expected);

    drop(filter);
    drop(consensus_cmd);
    drop(protocol_cmd);
    drop(network_cmd);
    consensus_join_handle.await.unwrap();
    protocol_join_handle.await.unwrap();
    network_join_handle.await.unwrap();
}

#[tokio::test]
async fn test_get_graph_interval() {
    let mut mock_consensus_data = ConsensusMockData::new();
    mock_consensus_data.graph.best_parents = vec![get_test_hash(), get_test_hash()];

    mock_consensus_data.add_active_blocks(get_test_hash(), get_test_block());

    mock_consensus_data.dummy_signature= crypto::signature::Signature::from_bs58_check(
        "5f4E3opXPWc3A1gvRVV7DJufvabDfaLkT1GMterpJXqRZ5B7bxPe5LoNzGDQp9LkphQuChBN1R5yEvVJqanbjx7mgLEae"
    ).unwrap();

    let (consensus_join_handle, consensus_cmd) = start_mock_consensus(mock_consensus_data);
    let (protocol_join_handle, protocol_cmd) = start_mock_protocol();
    let (network_join_handle, network_cmd) = start_mock_network(NetworkMockData::new());

    let filter = mock_filter(
        consensus_cmd.clone(),
        protocol_cmd.clone(),
        network_cmd.clone(),
    );

    // invalid hash: filter mismatch
    let matches = warp::test::request()
        .method("GET")
        .path(&"/api/v1/graph_interval")
        .matches(&filter)
        .await;
    assert!(!matches);

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
    let obtained: serde_json::Value = serde_json::from_slice(res.body()).unwrap();
    let block = get_test_block();
    let expected = vec![(
        get_test_hash(),
        block.header.period_number,
        block.header.thread_number,
        "final", // in tests there are no blocks in gi_head, so no just active blocks
        block.header.parents,
    )];
    let expected: serde_json::Value =
        serde_json::from_str(&serde_json::to_string(&expected).unwrap()).unwrap();
    assert_eq!(obtained, expected);

    drop(filter);
    drop(consensus_cmd);
    drop(protocol_cmd);
    drop(network_cmd);
    consensus_join_handle.await.unwrap();
    protocol_join_handle.await.unwrap();
    network_join_handle.await.unwrap();
}

#[tokio::test]
async fn test_last_final() {
    //test with empty final block
    {
        let (consensus_join_handle, consensus_cmd) = start_mock_consensus(ConsensusMockData::new());
        let (protocol_join_handle, protocol_cmd) = start_mock_protocol();
        let (network_join_handle, network_cmd) = start_mock_network(NetworkMockData::new());

        let filter = mock_filter(
            consensus_cmd.clone(),
            protocol_cmd.clone(),
            network_cmd.clone(),
        );
        let res = warp::test::request()
            .method("GET")
            .path("/api/v1/last_final")
            .reply(&filter)
            .await;
        assert_eq!(res.status(), 200);
        let obtained: serde_json::Value = serde_json::from_slice(res.body()).unwrap();
        let expected: serde_json::Value =
            serde_json::from_str(&serde_json::to_string(&Vec::<(Hash, u64, u8)>::new()).unwrap())
                .unwrap();
        assert_eq!(obtained, expected);

        drop(filter);
        drop(consensus_cmd);
        drop(protocol_cmd);
        drop(network_cmd);
        consensus_join_handle.await.unwrap();
        protocol_join_handle.await.unwrap();
        network_join_handle.await.unwrap();
    }

    //add default final blocks
    let mut mock_consensus_data = ConsensusMockData::new();
    mock_consensus_data.graph.latest_final_blocks_periods =
        vec![(get_test_hash(), 10), (get_test_hash(), 21)];

    let (consensus_join_handle, consensus_cmd) = start_mock_consensus(mock_consensus_data);
    let (protocol_join_handle, protocol_cmd) = start_mock_protocol();
    let (network_join_handle, network_cmd) = start_mock_network(NetworkMockData::new());

    let filter = mock_filter(
        consensus_cmd.clone(),
        protocol_cmd.clone(),
        network_cmd.clone(),
    );
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
        &serde_json::to_string(&vec![(get_test_hash(), 10, 0), (get_test_hash(), 21, 1)]).unwrap(),
    )
    .unwrap();
    assert_eq!(obtained, expected);

    drop(filter);
    drop(consensus_cmd);
    drop(protocol_cmd);
    drop(network_cmd);
    consensus_join_handle.await.unwrap();
    protocol_join_handle.await.unwrap();
    network_join_handle.await.unwrap();
}

#[tokio::test]
async fn test_peers() {
    //test with empty final peers
    {
        let (consensus_join_handle, consensus_cmd) = start_mock_consensus(ConsensusMockData::new());
        let (protocol_join_handle, protocol_cmd) = start_mock_protocol();
        let (network_join_handle, network_cmd) = start_mock_network(NetworkMockData::new());

        let filter = mock_filter(
            consensus_cmd.clone(),
            protocol_cmd.clone(),
            network_cmd.clone(),
        );
        let res = warp::test::request()
            .method("GET")
            .path("/api/v1/peers")
            .reply(&filter)
            .await;
        assert_eq!(res.status(), 200);
        let obtained: serde_json::Value = serde_json::from_slice(res.body()).unwrap();
        let expected: serde_json::Value = serde_json::from_str(
            &serde_json::to_string(&HashMap::<IpAddr, String>::new()).unwrap(),
        )
        .unwrap();
        assert_eq!(obtained, expected);

        drop(filter);
        drop(consensus_cmd);
        drop(protocol_cmd);
        drop(network_cmd);
        consensus_join_handle.await.unwrap();
        protocol_join_handle.await.unwrap();
        network_join_handle.await.unwrap();
    }

    //add default peers
    let mut mock_network_data = NetworkMockData::new();

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
    mock_network_data.peers = peers.clone();

    let (consensus_join_handle, consensus_cmd) = start_mock_consensus(ConsensusMockData::new());
    let (protocol_join_handle, protocol_cmd) = start_mock_protocol();
    let (network_join_handle, network_cmd) = start_mock_network(mock_network_data);

    let filter = mock_filter(
        consensus_cmd.clone(),
        protocol_cmd.clone(),
        network_cmd.clone(),
    );

    // invalid url parameter
    let matches = warp::test::request()
        .method("GET")
        .path(&"/api/v1/peers/123")
        .matches(&filter)
        .await;
    println!("matches:{:?}", matches);
    assert!(!matches);

    //valide url with peers.
    let res = warp::test::request()
        .method("GET")
        .path("/api/v1/peers")
        .reply(&filter)
        .await;
    assert_eq!(res.status(), 200);
    let obtained: serde_json::Value = serde_json::from_slice(res.body()).unwrap();
    let expected: serde_json::Value =
        serde_json::from_str(&serde_json::to_string(&peers).unwrap()).unwrap();
    assert_eq!(obtained, expected);

    drop(filter);
    drop(consensus_cmd);
    drop(protocol_cmd);
    drop(network_cmd);
    consensus_join_handle.await.unwrap();
    protocol_join_handle.await.unwrap();
    network_join_handle.await.unwrap();
}

#[tokio::test]
async fn test_get_block_interval() {
    let mut mock_consensus_data = ConsensusMockData::new();
    mock_consensus_data.graph.best_parents = vec![get_test_hash(), get_test_hash()];

    mock_consensus_data.add_active_blocks(get_test_hash(), get_test_block());

    mock_consensus_data.dummy_signature = crypto::signature::Signature::from_bs58_check(
        "5f4E3opXPWc3A1gvRVV7DJufvabDfaLkT1GMterpJXqRZ5B7bxPe5LoNzGDQp9LkphQuChBN1R5yEvVJqanbjx7mgLEae"
    ).unwrap();

    let (consensus_join_handle, consensus_cmd) = start_mock_consensus(mock_consensus_data);
    let (protocol_join_handle, protocol_cmd) = start_mock_protocol();
    let (network_join_handle, network_cmd) = start_mock_network(NetworkMockData::new());

    let filter = mock_filter(
        consensus_cmd.clone(),
        protocol_cmd.clone(),
        network_cmd.clone(),
    );

    // invalid hash: filter mismatch
    let matches = warp::test::request()
        .method("GET")
        .path(&"/api/v1/blockinterval")
        .matches(&filter)
        .await;
    assert!(!matches);

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
        serde_json::from_str(&serde_json::to_string(&Vec::<(Hash, (u64, u8))>::new()).unwrap())
            .unwrap();
    assert_eq!(obtained, expected);

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
    expected.push((
        get_test_hash(),
        (
            get_test_block().header.period_number,
            get_test_block().header.thread_number,
        ),
    ));
    let expected: serde_json::Value =
        serde_json::from_str(&serde_json::to_string(&expected).unwrap()).unwrap();
    assert_eq!(obtained, expected);

    drop(filter);
    drop(consensus_cmd);
    drop(protocol_cmd);
    drop(network_cmd);
    consensus_join_handle.await.unwrap();
    protocol_join_handle.await.unwrap();
    network_join_handle.await.unwrap();
}

#[tokio::test]
async fn test_get_block() {
    let mut mock_consensus_data = ConsensusMockData::new();
    mock_consensus_data.graph.best_parents = vec![get_test_hash(), get_test_hash()];

    mock_consensus_data.add_active_blocks(get_test_hash(), get_test_block());

    mock_consensus_data.dummy_signature=  crypto::signature::Signature::from_bs58_check(
        "5f4E3opXPWc3A1gvRVV7DJufvabDfaLkT1GMterpJXqRZ5B7bxPe5LoNzGDQp9LkphQuChBN1R5yEvVJqanbjx7mgLEae"
    ).unwrap();

    let (consensus_join_handle, consensus_cmd) = start_mock_consensus(mock_consensus_data);
    let (protocol_join_handle, protocol_cmd) = start_mock_protocol();
    let (network_join_handle, network_cmd) = start_mock_network(NetworkMockData::new());

    let filter = mock_filter(
        consensus_cmd.clone(),
        protocol_cmd.clone(),
        network_cmd.clone(),
    );

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

    drop(filter);
    drop(consensus_cmd);
    drop(protocol_cmd);
    drop(network_cmd);
    consensus_join_handle.await.unwrap();
    protocol_join_handle.await.unwrap();
    network_join_handle.await.unwrap();
}

#[tokio::test]
async fn test_network_info() {
    //test with empty peer list
    {
        let (consensus_join_handle, consensus_cmd) = start_mock_consensus(ConsensusMockData::new());
        let (protocol_join_handle, protocol_cmd) = start_mock_protocol();
        let (network_join_handle, network_cmd) = start_mock_network(NetworkMockData::new());

        let filter = mock_filter(
            consensus_cmd.clone(),
            protocol_cmd.clone(),
            network_cmd.clone(),
        );
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

        drop(filter);
        drop(consensus_cmd);
        drop(protocol_cmd);
        drop(network_cmd);
        consensus_join_handle.await.unwrap();
        protocol_join_handle.await.unwrap();
        network_join_handle.await.unwrap();
    }

    //add default peers
    let mut network_mock_data = NetworkMockData::new();

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
    network_mock_data.peers = peers.clone();

    let (consensus_join_handle, consensus_cmd) = start_mock_consensus(ConsensusMockData::new());
    let (protocol_join_handle, protocol_cmd) = start_mock_protocol();
    let (network_join_handle, network_cmd) = start_mock_network(network_mock_data);

    let filter = mock_filter(
        consensus_cmd.clone(),
        protocol_cmd.clone(),
        network_cmd.clone(),
    );

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

    drop(filter);
    drop(consensus_cmd);
    drop(protocol_cmd);
    drop(network_cmd);
    consensus_join_handle.await.unwrap();
    protocol_join_handle.await.unwrap();
    network_join_handle.await.unwrap();
}

#[tokio::test]
async fn test_state() {
    //test with empty final peers
    {
        let (consensus_join_handle, consensus_cmd) = start_mock_consensus(ConsensusMockData::new());
        let (protocol_join_handle, protocol_cmd) = start_mock_protocol();
        let (network_join_handle, network_cmd) = start_mock_network(NetworkMockData::new());

        let filter = mock_filter(
            consensus_cmd.clone(),
            protocol_cmd.clone(),
            network_cmd.clone(),
        );

        let res = warp::test::request()
            .method("GET")
            .path("/api/v1/state")
            .reply(&filter)
            .await;
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
        drop(consensus_cmd);
        drop(protocol_cmd);
        drop(network_cmd);
        consensus_join_handle.await.unwrap();
        protocol_join_handle.await.unwrap();
        network_join_handle.await.unwrap();
    }

    //add default peers
    let mut network_mock_data = NetworkMockData::new();

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
    network_mock_data.peers = peers.clone();

    let (consensus_join_handle, consensus_cmd) = start_mock_consensus(ConsensusMockData::new());
    let (protocol_join_handle, protocol_cmd) = start_mock_protocol();
    let (network_join_handle, network_cmd) = start_mock_network(network_mock_data);

    let filter = mock_filter(
        consensus_cmd.clone(),
        protocol_cmd.clone(),
        network_cmd.clone(),
    );

    let consensus_cfg = get_consensus_config();

    let _latest_block_slot = consensus::get_latest_block_slot_at_timestamp(
        consensus_cfg.thread_count,
        consensus_cfg.t0,
        consensus_cfg.genesis_timestamp,
        UTime::now().unwrap(),
    )
    .unwrap()
    .unwrap();

    let res = warp::test::request()
        .method("GET")
        .path("/api/v1/state")
        .reply(&filter)
        .await;
    assert_eq!(res.status(), 200);
    if let Ok(serde_json::Value::Object(obtained_map)) = serde_json::from_slice(res.body()) {
        assert_eq!(obtained_map["last_final"], json! {[]});
        assert_eq!(obtained_map["nb_cliques"], json! {0});
        assert_eq!(obtained_map["nb_peers"], json! {peers.len()});
        assert_eq!(obtained_map["our_ip"], json! {"127.0.0.1"});
    } else {
        panic!("wrong root object type");
    }

    drop(filter);
    drop(consensus_cmd);
    drop(protocol_cmd);
    drop(network_cmd);
    consensus_join_handle.await.unwrap();
    protocol_join_handle.await.unwrap();
    network_join_handle.await.unwrap();
}

#[tokio::test]
async fn test_last_stale() {
    //test with empty final block
    {
        let (consensus_join_handle, consensus_cmd) = start_mock_consensus(ConsensusMockData::new());
        let (protocol_join_handle, protocol_cmd) = start_mock_protocol();
        let (network_join_handle, network_cmd) = start_mock_network(NetworkMockData::new());

        let filter = mock_filter(
            consensus_cmd.clone(),
            protocol_cmd.clone(),
            network_cmd.clone(),
        );
        let res = warp::test::request()
            .method("GET")
            .path("/api/v1/last_stale")
            .reply(&filter)
            .await;
        assert_eq!(res.status(), 200);
        let obtained: serde_json::Value = serde_json::from_slice(res.body()).unwrap();
        let expected: serde_json::Value =
            serde_json::from_str(&serde_json::to_string(&Vec::<(Hash, u64, u8)>::new()).unwrap())
                .unwrap();
        assert_eq!(obtained, expected);

        drop(filter);
        drop(consensus_cmd);
        drop(protocol_cmd);
        drop(network_cmd);
        consensus_join_handle.await.unwrap();
        protocol_join_handle.await.unwrap();
        network_join_handle.await.unwrap();
    }

    //add default stale blocks
    let mut consensus_mock_data = ConsensusMockData::new();
    consensus_mock_data.graph.discarded_blocks.map.extend(vec![
        (
            get_test_hash(),
            (DiscardReason::Invalid, get_header(1, 1, None)),
        ),
        (
            get_another_test_hash(),
            (DiscardReason::Stale, get_header(2, 0, None)),
        ),
    ]);

    let (consensus_join_handle, consensus_cmd) = start_mock_consensus(consensus_mock_data);
    let (protocol_join_handle, protocol_cmd) = start_mock_protocol();
    let (network_join_handle, network_cmd) = start_mock_network(NetworkMockData::new());

    let filter = mock_filter(
        consensus_cmd.clone(),
        protocol_cmd.clone(),
        network_cmd.clone(),
    );

    //valide url with final block.
    let res = warp::test::request()
        .method("GET")
        .path("/api/v1/last_stale")
        .reply(&filter)
        .await;
    assert_eq!(res.status(), 200);
    let obtained: serde_json::Value = serde_json::from_slice(res.body()).unwrap();
    let expected: serde_json::Value = serde_json::from_str(
        &serde_json::to_string(&vec![(get_another_test_hash(), 2, 0)]).unwrap(),
    )
    .unwrap();
    assert_eq!(obtained, expected);

    drop(filter);
    drop(consensus_cmd);
    drop(protocol_cmd);
    drop(network_cmd);
    consensus_join_handle.await.unwrap();
    protocol_join_handle.await.unwrap();
    network_join_handle.await.unwrap();
}

#[tokio::test]
async fn test_last_invalid() {
    //test with empty final block
    {
        let (consensus_join_handle, consensus_cmd) = start_mock_consensus(ConsensusMockData::new());
        let (protocol_join_handle, protocol_cmd) = start_mock_protocol();
        let (network_join_handle, network_cmd) = start_mock_network(NetworkMockData::new());

        let filter = mock_filter(
            consensus_cmd.clone(),
            protocol_cmd.clone(),
            network_cmd.clone(),
        );
        let res = warp::test::request()
            .method("GET")
            .path("/api/v1/last_invalid")
            .reply(&filter)
            .await;
        assert_eq!(res.status(), 200);
        let obtained: serde_json::Value = serde_json::from_slice(res.body()).unwrap();
        let expected: serde_json::Value =
            serde_json::from_str(&serde_json::to_string(&Vec::<(Hash, u64, u8)>::new()).unwrap())
                .unwrap();
        assert_eq!(obtained, expected);

        drop(filter);
        drop(consensus_cmd);
        drop(protocol_cmd);
        drop(network_cmd);
        consensus_join_handle.await.unwrap();
        protocol_join_handle.await.unwrap();
        network_join_handle.await.unwrap();
    }

    //add default stale blocks
    let mut consensus_mock_data = ConsensusMockData::new();
    consensus_mock_data.graph.discarded_blocks.map.extend(vec![
        (
            get_test_hash(),
            (DiscardReason::Invalid, get_header(1, 1, None)),
        ),
        (
            get_another_test_hash(),
            (DiscardReason::Stale, get_header(2, 0, None)),
        ),
    ]);

    let (consensus_join_handle, consensus_cmd) = start_mock_consensus(consensus_mock_data);
    let (protocol_join_handle, protocol_cmd) = start_mock_protocol();
    let (network_join_handle, network_cmd) = start_mock_network(NetworkMockData::new());

    let filter = mock_filter(
        consensus_cmd.clone(),
        protocol_cmd.clone(),
        network_cmd.clone(),
    );

    //valide url with final block.
    let res = warp::test::request()
        .method("GET")
        .path("/api/v1/last_invalid")
        .reply(&filter)
        .await;
    assert_eq!(res.status(), 200);
    let obtained: serde_json::Value = serde_json::from_slice(res.body()).unwrap();
    let expected = serde_json::to_value(&vec![(get_test_hash(), 1, 1)]).unwrap();
    assert_eq!(obtained, expected);

    drop(filter);
    drop(consensus_cmd);
    drop(protocol_cmd);
    drop(network_cmd);
    consensus_join_handle.await.unwrap();
    protocol_join_handle.await.unwrap();
    network_join_handle.await.unwrap();
}

#[tokio::test]
async fn test_staker_info() {
    //test with empty final block
    {
        let consensus_mock_data = ConsensusMockData::new();
        let (consensus_join_handle, consensus_cmd) =
            start_mock_consensus(consensus_mock_data.clone());
        let (protocol_join_handle, protocol_cmd) = start_mock_protocol();
        let (network_join_handle, network_cmd) = start_mock_network(NetworkMockData::new());

        let filter = mock_filter(
            consensus_cmd.clone(),
            protocol_cmd.clone(),
            network_cmd.clone(),
        );

        let staker = consensus_mock_data.dummy_creator;
        let res = warp::test::request()
            .method("GET")
            .path(&format!("/api/v1/staker_info/{}", staker))
            .reply(&filter)
            .await;
        assert_eq!(res.status(), 200);
        let obtained: serde_json::Value = serde_json::from_slice(res.body()).unwrap();
        let empty_vec = serde_json::to_value(&Vec::<Block>::new()).unwrap();
        assert_eq!(obtained["staker_active_blocks"], empty_vec);
        assert_eq!(obtained["staker_discarded_blocks"], empty_vec);
        assert_eq!(
            obtained["staker_next_draws"],
            serde_json::to_value(vec![(0u64, 0u8)]).unwrap()
        );

        drop(filter);
        drop(consensus_cmd);
        drop(protocol_cmd);
        drop(network_cmd);
        consensus_join_handle.await.unwrap();
        protocol_join_handle.await.unwrap();
        network_join_handle.await.unwrap();
    }
    //add default stale blocks
    let mut consensus_mock_data = ConsensusMockData::new();
    let staker = consensus_mock_data.dummy_creator;

    let staker_s_discarded = vec![(
        get_test_hash(),
        (DiscardReason::Invalid, get_header(1, 1, Some(staker))),
    )];
    consensus_mock_data.graph.discarded_blocks.map.extend(vec![
        staker_s_discarded[0].clone(),
        (
            get_another_test_hash(),
            (DiscardReason::Stale, get_header(2, 0, None)),
        ),
    ]);

    let staker_s_active = vec![(
        get_another_test_hash(),
        get_test_compiled_exported_block(2, 1, Some(staker)),
    )];
    consensus_mock_data
        .graph
        .active_blocks
        .insert(staker_s_active[0].0.clone(), staker_s_active[0].1.clone());

    let (consensus_join_handle, consensus_cmd) = start_mock_consensus(consensus_mock_data);
    let (protocol_join_handle, protocol_cmd) = start_mock_protocol();
    let (network_join_handle, network_cmd) = start_mock_network(NetworkMockData::new());

    let filter = mock_filter(
        consensus_cmd.clone(),
        protocol_cmd.clone(),
        network_cmd.clone(),
    );

    //valide url with final block.
    let res = warp::test::request()
        .method("GET")
        .path(&format!("/api/v1/staker_info/{}", staker))
        .reply(&filter)
        .await;
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
    drop(consensus_cmd);
    drop(protocol_cmd);
    drop(network_cmd);
    consensus_join_handle.await.unwrap();
    protocol_join_handle.await.unwrap();
    network_join_handle.await.unwrap();
}
