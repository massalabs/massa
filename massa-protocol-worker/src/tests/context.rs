use std::{collections::HashMap, fs::read_to_string, sync::Arc};

use crate::{
    connectivity::start_connectivity_thread, create_protocol_controller,
    handlers::peer_handler::models::PeerDB, manager::ProtocolManagerImpl,
    messages::MessagesHandler, tests::mock_network::MockNetworkController,
};
use massa_channel::MassaChannel;
use massa_consensus_exports::{ConsensusController, MockConsensusController};
use massa_metrics::MassaMetrics;
use massa_models::config::MIP_STORE_STATS_BLOCK_CONSIDERED;
//use crate::handlers::block_handler::BlockInfoReply;
use massa_pool_exports::{MockPoolController, PoolController};
use massa_pos_exports::{MockSelectorController, SelectorController};
use massa_protocol_exports::{
    PeerCategoryInfo, PeerId, ProtocolConfig, ProtocolController, ProtocolError, ProtocolManager,
};
use massa_serialization::U64VarIntDeserializer;
use massa_signature::KeyPair;
use massa_storage::Storage;
use massa_versioning::versioning::{MipStatsConfig, MipStore};
use num::rational::Ratio;
use parking_lot::RwLock;
use std::ops::Bound::Included;
use tracing::{debug, log::warn};

/// start a new `ProtocolController` from a `ProtocolConfig`
///
/// # Arguments
/// * `config`: protocol settings
/// * `consensus_controller`: interact with consensus module
/// * `storage`: Shared storage to fetch data that are fetch across all modules
pub fn start_protocol_controller_with_mock_network(
    config: ProtocolConfig,
    selector_controller: Box<dyn SelectorController>,
    consensus_controller: Box<dyn ConsensusController>,
    pool_controller: Box<dyn PoolController>,
    storage: Storage,
) -> Result<
    (
        Box<MockNetworkController>,
        Box<dyn ProtocolController>,
        Box<dyn ProtocolManager>,
    ),
    ProtocolError,
> {
    // try to read node keypair from file, otherwise generate it & write to file. Then derive nodeId
    let keypair = if std::path::Path::is_file(&config.keypair_file) {
        // file exists: try to load it
        let keypair_bs58_check_encoded = read_to_string(&config.keypair_file).map_err(|err| {
            std::io::Error::new(err.kind(), format!("could not load node key file: {}", err))
        })?;
        serde_json::from_slice::<KeyPair>(keypair_bs58_check_encoded.as_bytes())?
    } else {
        // node file does not exist: generate the key and save it
        let keypair = KeyPair::generate(0).unwrap();
        if let Err(e) = std::fs::write(&config.keypair_file, serde_json::to_string(&keypair)?) {
            warn!("could not generate node key file: {}", e);
        }
        keypair
    };
    debug!("starting protocol controller with mock network");
    let peer_db = Arc::new(RwLock::new(PeerDB::default()));

    let (sender_operations, receiver_operations) = MassaChannel::new(
        "operations".to_string(),
        Some(config.max_size_channel_network_to_operation_handler),
    );
    let (sender_endorsements, receiver_endorsements) = MassaChannel::new(
        "endorsements".to_string(),
        Some(config.max_size_channel_network_to_endorsement_handler),
    );
    let (sender_blocks, receiver_blocks) = MassaChannel::new(
        "blocks".to_string(),
        Some(config.max_size_channel_network_to_block_handler),
    );
    let (sender_peers, receiver_peers) = MassaChannel::new(
        "peers".to_string(),
        Some(config.max_size_channel_network_to_peer_handler),
    );

    // Register channels for handlers
    let message_handlers: MessagesHandler = MessagesHandler {
        sender_blocks: sender_blocks.clone(),
        sender_endorsements: sender_endorsements.clone(),
        sender_operations: sender_operations.clone(),
        sender_peers: sender_peers.clone(),
        id_deserializer: U64VarIntDeserializer::new(Included(0), Included(u64::MAX)),
    };

    let (controller, channels) = create_protocol_controller(config.clone());

    let network_controller = Box::new(MockNetworkController::new(
        message_handlers.clone(),
        peer_db.clone(),
    ));

    let mip_stats_config = MipStatsConfig {
        block_count_considered: MIP_STORE_STATS_BLOCK_CONSIDERED,
        warn_announced_version_ratio: Ratio::new_raw(30, 100),
    };
    let mip_store = MipStore::try_from(([], mip_stats_config)).unwrap();

    let connectivity_thread_handle = start_connectivity_thread(
        PeerId::from_public_key(keypair.get_public_key()),
        selector_controller,
        network_controller.clone(),
        consensus_controller,
        pool_controller,
        (sender_blocks, receiver_blocks),
        (sender_endorsements, receiver_endorsements),
        (sender_operations, receiver_operations),
        (sender_peers, receiver_peers),
        HashMap::default(),
        peer_db,
        storage,
        channels,
        message_handlers,
        HashMap::default(),
        PeerCategoryInfo {
            allow_local_peers: true,
            max_in_connections: 10,
            target_out_connections: 10,
            max_in_connections_per_ip: 10,
        },
        config,
        mip_store,
        MassaMetrics::new(
            false,
            "0.0.0.0:9898".parse().unwrap(),
            32,
            std::time::Duration::from_secs(5),
        )
        .0,
    )?;

    let manager = ProtocolManagerImpl::new(connectivity_thread_handle);

    Ok((network_controller, controller, Box::new(manager)))
}

pub fn protocol_test<F>(
    protocol_config: &ProtocolConfig,
    consensus_controller: Box<MockConsensusController>,
    pool_controller: Box<MockPoolController>,
    test: F,
) where
    F: FnOnce(Box<MockNetworkController>, Storage, Box<dyn ProtocolController>),
{
    let mut selector_controller = Box::new(MockSelectorController::new());
    selector_controller
        .expect_clone_box()
        .returning(|| Box::new(MockSelectorController::new()));
    let storage = Storage::create_root();
    // start protocol controller
    let (network_controller, protocol_controller, mut protocol_manager) =
        start_protocol_controller_with_mock_network(
            protocol_config.clone(),
            selector_controller,
            consensus_controller,
            pool_controller,
            storage.clone_without_refs(),
        )
        .expect("could not start protocol controller");

    test(network_controller, storage, protocol_controller);

    protocol_manager.stop()
}
