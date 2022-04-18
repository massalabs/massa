// Copyright (c) 2022 MASSA LABS <info@massa.net>
//! Bootstrap crate
//!
//! At start up, if now is after genesis timestamp,
//! the node will bootstrap from one of the provided bootstrap servers.
//!
//! On server side, the server will query consensus for the graph and the ledger,
//! execution for execution related data and network for the peer list.
//!
#![feature(async_closure)]
#![warn(missing_docs)]
#![warn(unused_crate_dependencies)]
#![feature(ip)]
use crate::client_binder::BootstrapClientBinder;
use crate::establisher::types::Duplex;
use crate::server_binder::BootstrapServerBinder;
use error::BootstrapError;
pub use establisher::types::Establisher;
use futures::{stream::FuturesUnordered, StreamExt};
use massa_consensus_exports::ConsensusCommandSender;
use massa_final_state::{FinalState, FinalStateBootstrap};
use massa_graph::BootstrapableGraph;
use massa_logging::massa_trace;
use massa_models::Version;
use massa_network_exports::{BootstrapPeers, NetworkCommandSender};
use massa_proof_of_stake_exports::ExportProofOfStake;
use massa_signature::{PrivateKey, PublicKey};
use massa_time::MassaTime;
use messages::BootstrapMessage;
use parking_lot::RwLock;
use rand::{prelude::SliceRandom, rngs::StdRng, SeedableRng};
use std::collections::{hash_map, HashMap};
use std::net::SocketAddr;
use std::sync::Arc;
use std::{convert::TryInto, net::IpAddr};
use tokio::time::Instant;
use tokio::{sync::mpsc, task::JoinHandle, time::sleep};
use tracing::{debug, info, warn};

mod client_binder;
mod error;
mod establisher;
mod messages;
mod server_binder;
mod settings;
pub use establisher::types;
pub use settings::BootstrapSettings;

#[cfg(test)]
pub mod tests;

/// a collection of the bootstrap state snapshots of all relevant modules
#[derive(Default, Debug)]
pub struct GlobalBootstrapState {
    /// state of the proof of stake state (distributions, seeds...)
    pub pos: Option<ExportProofOfStake>,

    /// state of the consensus graph
    pub graph: Option<BootstrapableGraph>,

    /// timestamp correction in milliseconds
    pub compensation_millis: i64,

    /// list of network peers
    pub peers: Option<BootstrapPeers>,

    /// state of the final state
    pub final_state: Option<FinalStateBootstrap>,
}

/// Gets the state from a bootstrap server (internal private function)
/// needs to be CANCELLABLE
async fn get_state_internal(
    cfg: &BootstrapSettings, // TODO: should be a &'static ... see #1848
    bootstrap_addr: &SocketAddr,
    bootstrap_public_key: &PublicKey,
    establisher: &mut Establisher,
    our_version: Version,
) -> Result<GlobalBootstrapState, BootstrapError> {
    massa_trace!("bootstrap.lib.get_state_internal", {});
    info!("Start bootstrapping from {}", bootstrap_addr);

    // connect
    let mut connector = establisher.get_connector(cfg.connect_timeout).await?; // cancellable
    let socket = connector.connect(*bootstrap_addr).await?; // cancellable
    let mut client = BootstrapClientBinder::new(socket, *bootstrap_public_key);

    // handshake
    let send_time_uncompensated = MassaTime::now()?;
    // client.handshake() is not cancel-safe but we drop the whole client object if cancelled => it's OK
    match tokio::time::timeout(cfg.write_timeout.into(), client.handshake()).await {
        Err(_) => {
            return Err(std::io::Error::new(
                std::io::ErrorKind::TimedOut,
                "bootstrap handshake timed out",
            )
            .into())
        }
        Ok(Err(e)) => return Err(e),
        Ok(Ok(_)) => {}
    }

    // compute ping
    let ping = MassaTime::now()?.saturating_sub(send_time_uncompensated);
    if ping > cfg.max_ping {
        return Err(BootstrapError::GeneralError(
            "bootstrap ping too high".into(),
        ));
    }

    // First, clock and version.
    // client.next() is not cancel-safe but we drop the whole client object if cancelled => it's OK
    let server_time = match tokio::time::timeout(cfg.read_timeout.into(), client.next()).await {
        Err(_) => {
            return Err(std::io::Error::new(
                std::io::ErrorKind::TimedOut,
                "bootstrap clock sync read timed out",
            )
            .into())
        }
        Ok(Err(e)) => return Err(e),
        Ok(Ok(BootstrapMessage::BootstrapTime {
            server_time,
            version,
        })) => {
            if !our_version.is_compatible(&version) {
                return Err(BootstrapError::IncompatibleVersionError(format!(
                    "remote is running incompatible version: {} (local node version: {})",
                    version, our_version
                )));
            }
            server_time
        }
        Ok(Ok(msg)) => return Err(BootstrapError::UnexpectedMessage(msg)),
    };

    let recv_time_uncompensated = MassaTime::now()?;

    // compute ping
    let ping = recv_time_uncompensated.saturating_sub(send_time_uncompensated);
    if ping > cfg.max_ping {
        return Err(BootstrapError::GeneralError(
            "bootstrap ping too high".into(),
        ));
    }

    // compute compensation
    let compensation_millis = if cfg.enable_clock_synchronization {
        let local_time_uncompensated =
            recv_time_uncompensated.checked_sub(ping.checked_div_u64(2)?)?;
        let compensation_millis = if server_time >= local_time_uncompensated {
            server_time
                .saturating_sub(local_time_uncompensated)
                .to_millis()
        } else {
            local_time_uncompensated
                .saturating_sub(server_time)
                .to_millis()
        };
        let compensation_millis: i64 = compensation_millis.try_into().map_err(|_| {
            BootstrapError::GeneralError("Failed to convert compensation time into i64".into())
        })?;
        debug!("Server clock compensation set to: {}", compensation_millis);
        compensation_millis
    } else {
        0
    };

    // Second, get peers
    // client.next() is not cancel-safe but we drop the whole client object if cancelled => it's OK
    let peers = match tokio::time::timeout(cfg.read_timeout.into(), client.next()).await {
        Err(_) => {
            return Err(std::io::Error::new(
                std::io::ErrorKind::TimedOut,
                "bootstrap peer read timed out",
            )
            .into())
        }
        Ok(Err(e)) => return Err(e),
        Ok(Ok(BootstrapMessage::BootstrapPeers { peers })) => peers,
        Ok(Ok(msg)) => return Err(BootstrapError::UnexpectedMessage(msg)),
    };

    // Third, get consensus state
    // client.next() is not cancel-safe but we drop the whole client object if cancelled => it's OK
    let (pos, graph) = match tokio::time::timeout(cfg.read_timeout.into(), client.next()).await {
        Err(_) => {
            return Err(std::io::Error::new(
                std::io::ErrorKind::TimedOut,
                "bootstrap state read timed out",
            )
            .into())
        }
        Ok(Err(e)) => return Err(e),
        Ok(Ok(BootstrapMessage::ConsensusState { pos, graph })) => (pos, graph),
        Ok(Ok(msg)) => return Err(BootstrapError::UnexpectedMessage(msg)),
    };

    // Fourth, get final state
    // client.next() is not cancel-safe but we drop the whole client object if cancelled => it's OK
    let final_state = match tokio::time::timeout(cfg.read_timeout.into(), client.next()).await {
        Err(_) => {
            return Err(std::io::Error::new(
                std::io::ErrorKind::TimedOut,
                "final state bootstrap read timed out",
            )
            .into())
        }
        Ok(Err(e)) => return Err(e),
        Ok(Ok(BootstrapMessage::FinalState { final_state })) => final_state,
        Ok(Ok(msg)) => return Err(BootstrapError::UnexpectedMessage(msg)),
    };

    info!("Successful bootstrap");

    Ok(GlobalBootstrapState {
        pos: Some(pos),
        graph: Some(graph),
        compensation_millis,
        peers: Some(peers),
        final_state: Some(final_state),
    })
}

/// Gets the state from a bootstrap server
/// needs to be CANCELLABLE
pub async fn get_state(
    bootstrap_settings: &'static BootstrapSettings,
    mut establisher: Establisher,
    version: Version,
    genesis_timestamp: MassaTime,
    end_timestamp: Option<MassaTime>,
) -> Result<GlobalBootstrapState, BootstrapError> {
    massa_trace!("bootstrap.lib.get_state", {});
    let now = MassaTime::now()?;
    // if we are before genesis, do not bootstrap
    if now < genesis_timestamp {
        massa_trace!("bootstrap.lib.get_state.init_from_scratch", {});
        return Ok(GlobalBootstrapState::default());
    }
    // we are after genesis => bootstrap
    massa_trace!("bootstrap.lib.get_state.init_from_others", {});
    if bootstrap_settings.bootstrap_list.is_empty() {
        return Err(BootstrapError::GeneralError(
            "no bootstrap nodes found in list".into(),
        ));
    }
    let mut shuffled_list = bootstrap_settings.bootstrap_list.clone();
    shuffled_list.shuffle(&mut StdRng::from_entropy());
    loop {
        for (addr, pub_key) in shuffled_list.iter() {
            if let Some(end) = end_timestamp {
                if MassaTime::now().expect("could not get now time") > end {
                    panic!("This episode has come to an end, please get the latest testnet node version to continue");
                }
            }
            match get_state_internal(bootstrap_settings, addr, pub_key, &mut establisher, version)
                .await  // cancellable
            {
                Err(e) => {
                    warn!("error while bootstrapping: {}", e);
                    sleep(bootstrap_settings.retry_delay.into()).await;
                }
                Ok(res) => {
                    return Ok(res)
                }
            }
        }
    }
}

/// handle on the bootstrap server
pub struct BootstrapManager {
    join_handle: JoinHandle<Result<(), BootstrapError>>,
    manager_tx: mpsc::Sender<()>,
}

impl BootstrapManager {
    /// stop the bootstrap server
    pub async fn stop(self) -> Result<(), BootstrapError> {
        massa_trace!("bootstrap.lib.stop", {});
        if self.manager_tx.send(()).await.is_err() {
            warn!("bootstrap server already dropped");
        }
        let _ = self.join_handle.await?;
        Ok(())
    }
}

#[allow(clippy::too_many_arguments)]
/// TODO merging the command senders into one channel structure may allow removing that allow
///
/// start a bootstrap server.
/// Once your node will be ready, you may want other to bootstrap from you.
pub async fn start_bootstrap_server(
    consensus_command_sender: ConsensusCommandSender,
    network_command_sender: NetworkCommandSender,
    final_state: Arc<RwLock<FinalState>>,
    bootstrap_settings: &'static BootstrapSettings,
    establisher: Establisher,
    private_key: PrivateKey,
    compensation_millis: i64,
    version: Version,
) -> Result<Option<BootstrapManager>, BootstrapError> {
    massa_trace!("bootstrap.lib.start_bootstrap_server", {});
    if let Some(bind) = bootstrap_settings.bind {
        let (manager_tx, manager_rx) = mpsc::channel::<()>(1);
        let join_handle = tokio::spawn(async move {
            BootstrapServer {
                consensus_command_sender,
                network_command_sender,
                final_state,
                establisher,
                manager_rx,
                bind,
                private_key,
                compensation_millis,
                version,
                ip_hist_map: HashMap::with_capacity(bootstrap_settings.ip_list_max_size),
                bootstrap_settings,
            }
            .run()
            .await
        });
        Ok(Some(BootstrapManager {
            join_handle,
            manager_tx,
        }))
    } else {
        Ok(None)
    }
}

struct BootstrapServer {
    consensus_command_sender: ConsensusCommandSender,
    network_command_sender: NetworkCommandSender,
    final_state: Arc<RwLock<FinalState>>,
    establisher: Establisher,
    manager_rx: mpsc::Receiver<()>,
    bind: SocketAddr,
    private_key: PrivateKey,
    bootstrap_settings: &'static BootstrapSettings,
    compensation_millis: i64,
    version: Version,
    ip_hist_map: HashMap<IpAddr, Instant>,
}

impl BootstrapServer {
    pub async fn run(mut self) -> Result<(), BootstrapError> {
        debug!("starting bootstrap server");
        massa_trace!("bootstrap.lib.run", {});
        let mut listener = self.establisher.get_listener(self.bind).await?;
        let mut bootstrap_sessions = FuturesUnordered::new();
        let cache_timeout = self.bootstrap_settings.cache_duration.to_duration();
        let mut bootstrap_data: Option<(
            ExportProofOfStake,
            BootstrapableGraph,
            BootstrapPeers,
            FinalStateBootstrap,
        )> = None;
        let cache_timer = sleep(cache_timeout);
        let per_ip_min_interval = self.bootstrap_settings.per_ip_min_interval.to_duration();
        tokio::pin!(cache_timer);
        /*
            select! without the "biased" modifier will randomly select the 1st branch to check,
            then will check the next ones in the order they are written.
            We choose this order:
                * manager commands to avoid waiting too long to stop in case of contention
                * cache timeout to avoid skipping timeouts cleanup tasks (they are relatively rare)
                * bootstrap sessions (rare)
                * listener: most frequent => last
        */
        loop {
            massa_trace!("bootstrap.lib.run.select", {});
            tokio::select! {
                // managed commands
                _ = self.manager_rx.recv() => {
                    massa_trace!("bootstrap.lib.run.select.manager", {});
                    break
                },

                // cache cleanup timeout
                _ = &mut cache_timer, if bootstrap_data.is_some() => {
                    massa_trace!("bootstrap.lib.run.cache_unload", {});
                    bootstrap_data = None;
                }

                // bootstrap session finished
                Some(_) = bootstrap_sessions.next() => {
                    massa_trace!("bootstrap.session.finished", {"active_count": bootstrap_sessions.len()});
                }

                // listener
                Ok((dplx, remote_addr)) = listener.accept() => if bootstrap_sessions.len() < self.bootstrap_settings.max_simultaneous_bootstraps as usize {
                    massa_trace!("bootstrap.lib.run.select.accept", {"remote_addr": remote_addr});
                    let now = Instant::now();

                    // clear IP history if necessary
                    if self.ip_hist_map.len() > self.bootstrap_settings.ip_list_max_size {
                        self.ip_hist_map.retain(|_k, v| now.duration_since(*v) <= per_ip_min_interval);
                        if self.ip_hist_map.len() > self.bootstrap_settings.ip_list_max_size {
                            // too many IPs are spamming us: clear cache
                            warn!("high bootstrap load: at least {} different IPs attempted bootstrap in the last {}ms", self.ip_hist_map.len(), self.bootstrap_settings.per_ip_min_interval);
                            self.ip_hist_map.clear();
                        }
                    }

                    // check IP's bootstrap attempt history
                    match self.ip_hist_map.entry(remote_addr.ip()) {
                        hash_map::Entry::Occupied(mut occ) => {
                            if now.duration_since(*occ.get()) <= per_ip_min_interval {
                                // in list, non-expired => refuse
                                massa_trace!("bootstrap.lib.run.select.accept.refuse_limit", {"remote_addr": remote_addr});
                                continue;
                            } else {
                                // in list, expired
                                occ.insert(now);
                            }
                        },
                        hash_map::Entry::Vacant(vac) => {
                            vac.insert(now);
                        }
                    }

                    // load cache if absent
                    if bootstrap_data.is_none() {
                        massa_trace!("bootstrap.lib.run.select.accept.cache_load.start", {});

                        // Note that all requests are done simultaneously except for the consensus graph that is done after the others.
                        // This is done to ensure that the execution bootstrap state is older than the consensus state.
                        // If the consensus state snapshot is older than the execution state snapshot,
                        //   the execution final ledger will be in the future after bootstrap, which causes an inconsistency.
                        let peer_boot = self.network_command_sender.get_bootstrap_peers().await?;
                        let res_state = self.final_state.read().get_bootstrap_state();
                        let (pos_boot, graph_boot) = self.consensus_command_sender.get_bootstrap_state().await?;
                        bootstrap_data = Some((pos_boot, graph_boot, peer_boot, res_state));
                        cache_timer.set(sleep(cache_timeout));
                    }
                    massa_trace!("bootstrap.lib.run.select.accept.cache_available", {});

                    // launch bootstrap
                    let private_key = self.private_key;
                    let compensation_millis = self.compensation_millis;
                    let version = self.version;
                    let (data_pos, data_graph, data_peers, data_execution) = bootstrap_data.clone().unwrap(); // will not panic (checked above)
                    bootstrap_sessions.push(async move {
                        match manage_bootstrap(self.bootstrap_settings, dplx, data_pos, data_graph, data_peers, data_execution, private_key, compensation_millis, version).await {
                            Ok(_) => info!("bootstrapped peer {}", remote_addr),
                            Err(err) => debug!("bootstrap serving error for peer {}: {}", remote_addr, err),
                        }
                    });
                    massa_trace!("bootstrap.session.started", {"active_count": bootstrap_sessions.len()});
                } else {
                    debug!("did not bootstrap {}: no available slots", remote_addr);
                },
            }
        }

        // wait for bootstrap sessions to finish
        while bootstrap_sessions.next().await.is_some() {}

        Ok(())
    }
}

#[allow(clippy::too_many_arguments)]
async fn manage_bootstrap(
    bootstrap_settings: &'static BootstrapSettings,
    duplex: Duplex,
    data_pos: ExportProofOfStake,
    data_graph: BootstrapableGraph,
    data_peers: BootstrapPeers,
    final_state: FinalStateBootstrap,
    private_key: PrivateKey,
    compensation_millis: i64,
    version: Version,
) -> Result<(), BootstrapError> {
    massa_trace!("bootstrap.lib.manage_bootstrap", {});
    let mut server = BootstrapServerBinder::new(duplex, private_key);

    // Handshake
    send_state_timeout(
        bootstrap_settings.read_timeout.into(),
        server.handshake(),
        "bootstrap handshake send timed out",
    )
    .await?;

    let write_timeout: std::time::Duration = bootstrap_settings.write_timeout.into();

    // First, sync clocks.
    let server_time = MassaTime::compensated_now(compensation_millis)?;

    send_state_timeout(
        write_timeout,
        server.send(messages::BootstrapMessage::BootstrapTime {
            server_time,
            version,
        }),
        "bootstrap clock send timed out",
    )
    .await?;

    // Second, send peers
    send_state_timeout(
        write_timeout,
        server.send(messages::BootstrapMessage::BootstrapPeers { peers: data_peers }),
        "bootstrap clock send timed out",
    )
    .await?;

    // Third, send consensus state
    send_state_timeout(
        write_timeout,
        server.send(messages::BootstrapMessage::ConsensusState {
            pos: data_pos,
            graph: data_graph,
        }),
        "bootstrap graph send timed out",
    )
    .await?;

    // Fourth, send final state
    send_state_timeout(
        write_timeout,
        server.send(messages::BootstrapMessage::FinalState { final_state }),
        "bootstrap ledger state send timed out",
    )
    .await
}

/// Tooling, Send a future with a timeout, print error if timeout reached
async fn send_state_timeout(
    duration: std::time::Duration,
    future: impl futures::Future<Output = Result<(), BootstrapError>>,
    error: &str,
) -> Result<(), BootstrapError> {
    match tokio::time::timeout(duration, future).await {
        Err(_) => Err(std::io::Error::new(std::io::ErrorKind::TimedOut, error).into()),
        Ok(Err(e)) => Err(e),
        Ok(Ok(_)) => Ok(()),
    }
}
