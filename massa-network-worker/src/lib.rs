// Copyright (c) 2022 MASSA LABS <info@massa.net>

#![feature(async_closure)]
#![feature(drain_filter)]
#![feature(ip)]
#![warn(missing_docs)]
#![warn(unused_crate_dependencies)]
//! Manages a connection with a node

use crate::{
    network_worker::{NetworkWorker, NetworkWorkerChannels},
    peer_info_database::PeerInfoDatabase,
};
use crossbeam_channel::bounded;
use massa_logging::massa_trace;
use massa_models::{node::NodeId, version::Version};
use massa_network_exports::{
    BootstrapPeers, Establisher, NetworkCommand, NetworkCommandSender, NetworkConfig, NetworkError,
    NetworkEvent, NetworkEventReceiver, NetworkManagementCommand, NetworkManager,
};
use massa_signature::KeyPair;
use std::thread;
use tokio::runtime::Runtime;
use tracing::{debug, error, info, warn};

//pub use establisher::Establisher;
mod binders;
mod handshake_worker;
mod messages;
mod network_cmd_impl;
mod network_event;
mod network_worker;
mod node_worker;
mod peer_info_database;

#[cfg(test)]
pub mod tests;

/// Starts a new `NetworkWorker` in a spawned task
///
/// # Arguments
/// * `cfg`: network configuration
pub async fn start_network_controller(
    network_settings: &NetworkConfig,
    mut establisher: Establisher,
    initial_peers: Option<BootstrapPeers>,
    version: Version,
) -> Result<
    (
        NetworkCommandSender,
        NetworkEventReceiver,
        NetworkManager,
        KeyPair,
        NodeId,
    ),
    NetworkError,
> {
    debug!("starting network controller");

    // check that local IP is routable
    if let Some(self_ip) = network_settings.routable_ip {
        if !self_ip.is_global() {
            return Err(NetworkError::InvalidIpError(self_ip));
        }
    }

    // try to read node keypair from file, otherwise generate it & write to file. Then derive nodeId
    let keypair = if std::path::Path::is_file(&network_settings.keypair_file) {
        // file exists: try to load it
        let keypair_bs58_check_encoded = tokio::fs::read_to_string(&network_settings.keypair_file)
            .await
            .map_err(|err| {
                std::io::Error::new(err.kind(), format!("could not load node key file: {}", err))
            })?;
        serde_json::from_slice::<KeyPair>(keypair_bs58_check_encoded.as_bytes())?
    } else {
        // node file does not exist: generate the key and save it
        let keypair = KeyPair::generate();
        if let Err(e) = tokio::fs::write(
            &network_settings.keypair_file,
            serde_json::to_string(&keypair)?,
        )
        .await
        {
            warn!("could not generate node key file: {}", e);
        }
        keypair
    };
    let self_node_id = NodeId::new(keypair.get_public_key());

    info!("The node_id of this node is: {}", self_node_id);
    massa_trace!("self_node_id", { "node_id": self_node_id });

    // create listener
    let listener = establisher.get_listener(network_settings.bind).await?;

    let runtime = Runtime::new().expect("Failed to initialize networking runtime.");

    debug!("Loading peer database");
    // load peer info database
    let mut peer_info_db = PeerInfoDatabase::new(network_settings, runtime.handle().clone())?;

    // add bootstrap peers
    if let Some(peers) = initial_peers {
        peer_info_db.merge_candidate_peers(&peers.0)?;
    }

    // launch controller
    let (command_tx, controller_command_rx) =
        bounded::<NetworkCommand>(network_settings.controller_channel_size);
    let (controller_event_tx, event_rx) =
        bounded::<NetworkEvent>(network_settings.event_channel_size);
    let (manager_tx, controller_manager_rx) = bounded::<NetworkManagementCommand>(1);
    let cfg_copy = network_settings.clone();
    let keypair_cloned = keypair.clone();
    let join_handle = thread::spawn(move || {
        let res = NetworkWorker::new(
            cfg_copy,
            keypair_cloned,
            listener,
            establisher,
            peer_info_db,
            NetworkWorkerChannels {
                controller_command_rx,
                controller_event_tx,
                controller_manager_rx,
            },
            version,
            runtime,
        )
        .run_loop();
        match res {
            Err(err) => {
                error!("network worker crashed: {}", err);
                Err(err)
            }
            Ok(v) => {
                info!("network worker finished cleanly");
                Ok(v)
            }
        }
    });

    debug!("network controller started");

    Ok((
        NetworkCommandSender(command_tx),
        NetworkEventReceiver(event_rx),
        NetworkManager {
            join_handle,
            manager_tx,
        },
        keypair,
        self_node_id,
    ))
}
