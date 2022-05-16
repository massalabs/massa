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
use massa_logging::massa_trace;
use massa_models::{constants::CHANNEL_SIZE, node::NodeId, Version};
use massa_network_exports::{
    BootstrapPeers, Establisher, NetworkCommand, NetworkCommandSender, NetworkError, NetworkEvent,
    NetworkEventReceiver, NetworkManagementCommand, NetworkManager, NetworkSettings,
};
use massa_signature::{derive_public_key, generate_random_private_key, PrivateKey};
use massa_storage::Storage;
use tokio::sync::mpsc;
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
    network_settings: NetworkSettings,
    mut establisher: Establisher,
    clock_compensation: i64,
    initial_peers: Option<BootstrapPeers>,
    storage: Storage,
    version: Version,
) -> Result<
    (
        NetworkCommandSender,
        NetworkEventReceiver,
        NetworkManager,
        PrivateKey,
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

    // try to read node private key from file, otherwise generate it & write to file. Then derive nodeId
    let private_key = if std::path::Path::is_file(&network_settings.private_key_file) {
        // file exists: try to load it
        let private_key_bs58_check = tokio::fs::read_to_string(&network_settings.private_key_file)
            .await
            .map_err(|err| {
                std::io::Error::new(
                    err.kind(),
                    format!("could not load node private key file: {}", err),
                )
            })?;
        PrivateKey::from_bs58_check(private_key_bs58_check.trim()).map_err(|err| {
            std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                format!("node private key file corrupted: {}", err),
            )
        })?
    } else {
        // node file does not exist: generate the key and save it
        let priv_key = generate_random_private_key();
        if let Err(e) = tokio::fs::write(
            &network_settings.private_key_file,
            &priv_key.to_bs58_check(),
        )
        .await
        {
            warn!("could not generate node private key file: {}", e);
        }
        priv_key
    };
    let public_key = derive_public_key(&private_key);
    let self_node_id = NodeId(public_key);

    info!("The node_id of this node is: {}", self_node_id);
    massa_trace!("self_node_id", { "node_id": self_node_id });

    // create listener
    let listener = establisher.get_listener(network_settings.bind).await?;

    debug!("Loading peer database");
    // load peer info database
    let mut peer_info_db = PeerInfoDatabase::new(&network_settings, clock_compensation).await?;

    // add bootstrap peers
    if let Some(peers) = initial_peers {
        peer_info_db.merge_candidate_peers(&peers.0)?;
    }

    // launch controller
    let (command_tx, controller_command_rx) = mpsc::channel::<NetworkCommand>(CHANNEL_SIZE);
    let (controller_event_tx, event_rx) = mpsc::channel::<NetworkEvent>(CHANNEL_SIZE);
    let (manager_tx, controller_manager_rx) = mpsc::channel::<NetworkManagementCommand>(1);
    let cfg_copy = network_settings.clone();
    let join_handle = tokio::spawn(async move {
        let res = NetworkWorker::new(
            cfg_copy,
            private_key,
            listener,
            establisher,
            peer_info_db,
            NetworkWorkerChannels {
                controller_command_rx,
                controller_event_tx,
                controller_manager_rx,
            },
            storage,
            version,
        )
        .run_loop()
        .await;
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
        private_key,
        self_node_id,
    ))
}
