use crate::get_filter;
use async_trait::async_trait;
use communication::network::config::NetworkConfig;
use consensus::BlockGraphExport;
use consensus::ConsensusError;
use consensus::{
    config::ConsensusConfig, consensus_controller::ConsensusControllerInterface,
    ExportDiscardedBlocks,
};
use crypto::{hash::Hash, signature::PrivateKey};
use models::block::{Block, BlockHeader};
use serde_json::json;
use std::collections::{HashMap, HashSet, VecDeque};
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use time::UTime;
use warp::{filters::BoxedFilter, Reply};

#[derive(Debug, Clone)]
pub struct MockConsensusControllerInterface {
    pub graph: BlockGraphExport,
    pub peers: std::collections::HashMap<IpAddr, String>,
    pub dummy_signature: crypto::signature::Signature,
}

impl MockConsensusControllerInterface {
    pub fn new() -> MockConsensusControllerInterface {
        let signature_engine = crypto::signature::SignatureEngine::new();
        let private_key = crypto::signature::SignatureEngine::generate_random_private_key();
        let hash = Hash::hash("default_val".as_bytes());
        let dummy_signature = signature_engine
            .sign(&hash, &private_key)
            .expect("could not sign");

        let best_parents = vec![];
        let discarded_blocks = ExportDiscardedBlocks {
            map: HashMap::new(),
            vec_deque: VecDeque::new(),
            max_size: 0,
        };

        MockConsensusControllerInterface {
            graph: BlockGraphExport {
                genesis_blocks: Vec::new(),
                active_blocks: HashMap::new(),
                discarded_blocks,
                best_parents,
                latest_final_blocks_periods: Vec::new(),
                gi_head: HashMap::new(),
                max_cliques: Vec::new(),
            },
            peers: HashMap::new(),
            dummy_signature,
        }
    }

    pub fn add_active_blocks(&mut self, hash: Hash, block: Block) {
        self.graph.active_blocks.insert(
            hash,
            consensus::ExportCompiledBlock {
                block: block.header,
                children: vec![],
            },
        );
    }
}

#[async_trait]
impl ConsensusControllerInterface for MockConsensusControllerInterface {
    async fn get_block_graph_status(&self) -> Result<BlockGraphExport, ConsensusError> {
        Ok(self.graph.clone())
    }

    async fn get_active_block(&self, hash: Hash) -> Result<Option<Block>, ConsensusError> {
        //use a dummy signature

        Ok(self.graph.active_blocks.get(&hash).map(|cb| Block {
            header: cb.block.clone(),
            operations: vec![],
            signature: self.dummy_signature.clone(), // in a test
        }))
    }

    async fn get_peers(&self) -> Result<std::collections::HashMap<IpAddr, String>, ConsensusError> {
        Ok(self.peers.clone())
    }
}

fn get_consensus_config() -> ConsensusConfig {
    ConsensusConfig {
        genesis_timestamp: 0.into(),
        thread_count: 2,
        t0: 2000.into(),
        selection_rng_seed: 0,
        genesis_key: PrivateKey::from_bs58_check(
            "SGoTK5TJ9ZcCgQVmdfma88UdhS6GK94aFEYAsU3F1inFayQ6S",
        )
        .unwrap(),
        nodes: Vec::new(),
        current_node_index: 0,
        max_discarded_blocks: 0,
        future_block_processing_max_periods: 0,
        max_future_processing_blocks: 0,
        max_dependency_blocks: 0,
        delta_f0: 0,
        disable_block_creation: true,
    }
}

fn get_network_config() -> NetworkConfig {
    NetworkConfig {
        bind: SocketAddr::new(IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)), 8080),
        routable_ip: Some(IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1))),
        protocol_port: 0,
        connect_timeout: UTime::from(180_000),
        wakeup_interval: UTime::from(10_000),
        peers_file: std::path::PathBuf::new(),
        target_out_connections: 10,
        max_in_connections: 5,
        max_in_connections_per_ip: 2,
        max_out_connnection_attempts: 15,
        max_idle_peers: 3,
        max_banned_peers: 3,
        max_advertise_length: 5,
        peers_file_dump_interval: UTime::from(10_000),
    }
}
pub fn mock_filter(interface: MockConsensusControllerInterface) -> BoxedFilter<(impl Reply,)> {
    get_filter(interface, get_consensus_config(), get_network_config())
}

#[tokio::test]
async fn test_cliques() {
    {
        //test with no cliques
        let mock_interface = MockConsensusControllerInterface::new();
        let filter = mock_filter(mock_interface);
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
    }

    //add default cliques
    let mut mock_interface = MockConsensusControllerInterface::new();
    let hash_set = (0..2).map(|_| get_test_hash()).collect::<HashSet<Hash>>();
    mock_interface.graph.max_cliques = vec![hash_set.clone(), hash_set.clone()];

    let filter = mock_filter(mock_interface);
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
    let obtained: serde_json::Value = serde_json::from_slice(res.body()).unwrap();
    let expected: serde_json::Value = serde_json::from_str(
        &serde_json::to_string(&(2, vec![hash_set.clone(), hash_set.clone()])).unwrap(),
    )
    .unwrap();
    assert_eq!(obtained, expected);
}

fn get_test_hash() -> Hash {
    Hash::hash("test".as_bytes())
}

#[tokio::test]
async fn test_current_parents() {
    //test with empty parents
    {
        let mock_interface = MockConsensusControllerInterface::new();
        let filter = mock_filter(mock_interface);
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
    }

    //add default parents
    let mut mock_interface = MockConsensusControllerInterface::new();
    mock_interface.graph.best_parents = vec![get_test_hash(), get_test_hash()];

    let filter = mock_filter(mock_interface);
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
    let obtained: serde_json::Value = serde_json::from_slice(res.body()).unwrap();
    let expected: serde_json::Value = serde_json::from_str(
        &serde_json::to_string(&vec![get_test_hash(), get_test_hash()]).unwrap(),
    )
    .unwrap();
    assert_eq!(obtained, expected);
}

#[tokio::test]
async fn test_get_block_interval() {
    let mut mock_interface = MockConsensusControllerInterface::new();
    mock_interface.graph.best_parents = vec![get_test_hash(), get_test_hash()];

    mock_interface.add_active_blocks(get_test_hash(), get_test_block());

    mock_interface.dummy_signature=  crypto::signature::Signature::from_bs58_check(
                "5f4E3opXPWc3A1gvRVV7DJufvabDfaLkT1GMterpJXqRZ5B7bxPe5LoNzGDQp9LkphQuChBN1R5yEvVJqanbjx7mgLEae"
            ).unwrap();
    let filter = mock_filter(mock_interface);

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
        .path(&format!("/api/v1/graph_interval/{}/{}", start, end))
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
        .path(&format!("/api/v1/graph_interval/{}/{}", start, end))
        .reply(&filter)
        .await;
    assert_eq!(res.status(), 200);
    let obtained: serde_json::Value = serde_json::from_slice(res.body()).unwrap();
    let block = get_test_block();
    let expected = vec![(
        get_test_hash(),
        block.header.period_number,
        block.header.thread_number,
        "valid",
        block.header.parents,
    )];
    let expected: serde_json::Value =
        serde_json::from_str(&serde_json::to_string(&expected).unwrap()).unwrap();
    assert_eq!(obtained, expected);
}

#[tokio::test]
async fn test_last_final() {
    //test with empty final block
    {
        let mock_interface = MockConsensusControllerInterface::new();
        let filter = mock_filter(mock_interface);
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
    }

    //add default final blocks
    let mut mock_interface = MockConsensusControllerInterface::new();
    mock_interface.graph.latest_final_blocks_periods =
        vec![(get_test_hash(), 10), (get_test_hash(), 21)];

    let filter = mock_filter(mock_interface);
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
}

#[tokio::test]
async fn test_peers() {
    //test with empty final peers
    {
        let mock_interface = MockConsensusControllerInterface::new();
        let filter = mock_filter(mock_interface);
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
    }

    //add default peers
    let mut mock_interface = MockConsensusControllerInterface::new();

    let peers = (0..2)
        .map(|index| {
            (
                IpAddr::V4(Ipv4Addr::new(169, 202, 0, index)),
                index.to_string(),
            )
        })
        .collect::<HashMap<IpAddr, String>>();
    mock_interface.peers = peers.clone();
    let filter = mock_filter(mock_interface);
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
}

#[tokio::test]
async fn test_get_graph_interval() {
    let mut mock_interface = MockConsensusControllerInterface::new();
    mock_interface.graph.best_parents = vec![get_test_hash(), get_test_hash()];

    mock_interface.add_active_blocks(get_test_hash(), get_test_block());

    mock_interface.dummy_signature=  crypto::signature::Signature::from_bs58_check(
                "5f4E3opXPWc3A1gvRVV7DJufvabDfaLkT1GMterpJXqRZ5B7bxPe5LoNzGDQp9LkphQuChBN1R5yEvVJqanbjx7mgLEae"
            ).unwrap();
    let filter = mock_filter(mock_interface);

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
        .path(&format!("/api/v1/blockinterval/{}/{}", start, end))
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
        .path(&format!("/api/v1/blockinterval/{}/{}", start, end))
        .reply(&filter)
        .await;
    assert_eq!(res.status(), 200);
    let obtained: serde_json::Value = serde_json::from_slice(res.body()).unwrap();
    let expected: serde_json::Value =
        serde_json::from_str(&serde_json::to_string(&vec![get_test_hash()]).unwrap()).unwrap();
    assert_eq!(obtained, expected);
}

fn get_test_block() -> Block {
    Block {
            header: BlockHeader {
                creator: crypto::signature::PublicKey::from_bs58_check("4vYrPNzUM8PKg2rYPW3ZnXPzy67j9fn5WsGCbnwAnk2Lf7jNHb").unwrap(),
                endorsements: vec![],
                operation_merkle_root: get_test_hash(),
                out_ledger_hash: get_test_hash(),
                parents: vec![],
                period_number: 1,
                thread_number: 0,
                roll_number: 0,
            },
            operations: vec![],
            signature: crypto::signature::Signature::from_bs58_check(
                "5f4E3opXPWc3A1gvRVV7DJufvabDfaLkT1GMterpJXqRZ5B7bxPe5LoNzGDQp9LkphQuChBN1R5yEvVJqanbjx7mgLEae"
            ).unwrap()
        }
}

#[tokio::test]
async fn test_get_block() {
    let mut mock_interface = MockConsensusControllerInterface::new();
    mock_interface.graph.best_parents = vec![get_test_hash(), get_test_hash()];

    mock_interface.add_active_blocks(get_test_hash(), get_test_block());

    mock_interface.dummy_signature=  crypto::signature::Signature::from_bs58_check(
                "5f4E3opXPWc3A1gvRVV7DJufvabDfaLkT1GMterpJXqRZ5B7bxPe5LoNzGDQp9LkphQuChBN1R5yEvVJqanbjx7mgLEae"
            ).unwrap();
    let filter = mock_filter(mock_interface);

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
}

#[tokio::test]
async fn test_network_info() {
    //test with empty final peers
    {
        let mock_interface = MockConsensusControllerInterface::new();
        let filter = mock_filter(mock_interface);
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
    }

    //add default peers
    let mut mock_interface = MockConsensusControllerInterface::new();

    let peers = (0..2)
        .map(|index| {
            (
                IpAddr::V4(Ipv4Addr::new(169, 202, 0, index)),
                index.to_string(),
            )
        })
        .collect::<HashMap<IpAddr, String>>();
    mock_interface.peers = peers.clone();
    let filter = mock_filter(mock_interface);

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
}

#[tokio::test]
async fn test_state() {
    //test with empty final peers
    {
        let mock_interface = MockConsensusControllerInterface::new();
        let filter = mock_filter(mock_interface);
        let consensus_cfg = get_consensus_config();

        let time = UTime::now().unwrap();
        let time = consensus::get_latest_block_slot_at_timestamp(
            consensus_cfg.thread_count,
            consensus_cfg.t0,
            consensus_cfg.genesis_timestamp,
            time,
        )
        .unwrap()
        .unwrap();
        let res = warp::test::request()
            .method("GET")
            .path("/api/v1/state")
            .reply(&filter)
            .await;
        assert_eq!(res.status(), 200);
        let obtained: serde_json::Value = serde_json::from_slice(res.body()).unwrap();
        let expected: serde_json::Value = json!({
            "last_final": Vec::<Hash>::new(),
            "nb_cliques": 0,
            "nb_peers": 0,
            "our_ip": IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)),
            "time": time,
        });
        assert_eq!(obtained, expected);
    }

    //add default peers
    let mut mock_interface = MockConsensusControllerInterface::new();

    let peers = (0..2)
        .map(|index| {
            (
                IpAddr::V4(Ipv4Addr::new(169, 202, 0, index)),
                "connected".to_string(),
            )
        })
        .collect::<HashMap<IpAddr, String>>();
    mock_interface.peers = peers.clone();
    let filter = mock_filter(mock_interface);

    let consensus_cfg = get_consensus_config();
    let time = UTime::now().unwrap();
    let time = consensus::get_latest_block_slot_at_timestamp(
        consensus_cfg.thread_count,
        consensus_cfg.t0,
        consensus_cfg.genesis_timestamp,
        time,
    )
    .unwrap()
    .unwrap();
    let res = warp::test::request()
        .method("GET")
        .path("/api/v1/state")
        .reply(&filter)
        .await;
    assert_eq!(res.status(), 200);
    let obtained: serde_json::Value = serde_json::from_slice(res.body()).unwrap();
    let expected: serde_json::Value = json!({
        "last_final": Vec::<Hash>::new(),
        "nb_cliques": 0,
        "nb_peers": peers.len(),
        "our_ip": IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)),
        "time": time,
    });
    assert_eq!(obtained, expected);
}
