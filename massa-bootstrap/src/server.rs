use std::{
    collections::{hash_map, HashMap},
    net::{IpAddr, SocketAddr},
    sync::Arc,
    time::{Duration, Instant},
};

use futures::stream::FuturesUnordered;
use futures::StreamExt;
use massa_async_pool::AsyncMessageId;
use massa_consensus_exports::ConsensusCommandSender;
use massa_final_state::{FinalState, StateChanges};
use massa_graph::BootstrapableGraph;
use massa_ledger_exports::get_address_from_key;
use massa_logging::massa_trace;
use massa_models::{Slot, Version};
use massa_network_exports::{BootstrapPeers, NetworkCommandSender};
use massa_proof_of_stake_exports::ExportProofOfStake;
use massa_signature::KeyPair;
use massa_time::MassaTime;
use parking_lot::RwLock;
use tokio::{sync::mpsc, task::JoinHandle, time::sleep};
use tracing::{debug, info, warn};

use crate::{
    error::BootstrapError,
    messages::{BootstrapClientMessage, BootstrapServerMessage},
    server_binder::BootstrapServerBinder,
    BootstrapSettings, Establisher,
};

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
    keypair: KeyPair,
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
                keypair,
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
    keypair: KeyPair,
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
            Arc<RwLock<FinalState>>,
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
                Ok((dplx, remote_addr)) = listener.accept() => if bootstrap_sessions.len() < self.bootstrap_settings.max_simultaneous_bootstraps.try_into().map_err(|_| BootstrapError::GeneralError("Fail to convert u32 to usize".to_string()))? {
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
                                let mut server = BootstrapServerBinder::new(dplx, self.keypair.clone(), self.bootstrap_settings.max_bytes_read_write);
                                let _ = match tokio::time::timeout(self.bootstrap_settings.write_error_timeout.into(), server.send(BootstrapServerMessage::BootstrapError {
                                    error:
                                    format!("Your last bootstrap on this server was {:#?} ago and you have to wait {:#?} before retrying.", occ.get().elapsed(), per_ip_min_interval.saturating_sub(occ.get().elapsed()))
                                })).await {
                                    Err(_) => Err(std::io::Error::new(std::io::ErrorKind::TimedOut, "bootstrap error no available slots send timed out").into()),
                                    Ok(Err(e)) => Err(e),
                                    Ok(Ok(_)) => Ok(()),
                                };
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
                        let (pos_boot, graph_boot) = self.consensus_command_sender.get_bootstrap_state().await?;
                        bootstrap_data = Some((pos_boot, graph_boot, peer_boot, self.final_state.clone()));
                        cache_timer.set(sleep(cache_timeout));
                    }
                    massa_trace!("bootstrap.lib.run.select.accept.cache_available", {});

                    // launch bootstrap
                    let compensation_millis = self.compensation_millis;
                    let version = self.version;
                    let (data_pos, data_graph, data_peers, data_execution) = bootstrap_data.clone().unwrap(); // will not panic (checked above)
                    let keypair = self.keypair.clone();
                    bootstrap_sessions.push(async move {
                        //Socket lifetime
                        {
                            let mut server = BootstrapServerBinder::new(dplx, keypair, self.bootstrap_settings.max_bytes_read_write);
                            match manage_bootstrap(self.bootstrap_settings, &mut server, data_pos, data_graph, data_peers, data_execution, compensation_millis, version).await {
                                Ok(_) => info!("bootstrapped peer {}", remote_addr),
                                Err(BootstrapError::ReceivedError(error)) => debug!("bootstrap serving error received from peer {}: {}", remote_addr, error),
                                Err(err) => {
                                    debug!("bootstrap serving error for peer {}: {}", remote_addr, err);
                                    // We allow unused result because we don't care if an error is thrown when sending the error message to the server we will close the socket anyway.
                                    let _ = tokio::time::timeout(self.bootstrap_settings.write_error_timeout.into(), server.send(BootstrapServerMessage::BootstrapError { error: err.to_string() })).await;
                                },
                            }
                        }
                    });
                    massa_trace!("bootstrap.session.started", {"active_count": bootstrap_sessions.len()});
                } else {
                    let mut server = BootstrapServerBinder::new(dplx, self.keypair.clone(), self.bootstrap_settings.max_bytes_read_write);
                    let _ = match tokio::time::timeout(self.bootstrap_settings.write_error_timeout.into(), server.send(BootstrapServerMessage::BootstrapError {
                        error: "Bootstrap failed because the bootstrap server currently has no slots available.".to_string()
                    })).await {
                        Err(_) => Err(std::io::Error::new(std::io::ErrorKind::TimedOut, "bootstrap error no available slots send timed out").into()),
                        Ok(Err(e)) => Err(e),
                        Ok(Ok(_)) => Ok(()),
                    };
                    debug!("did not bootstrap {}: no available slots", remote_addr);
                }
            }
        }

        // wait for bootstrap sessions to finish
        while bootstrap_sessions.next().await.is_some() {}

        Ok(())
    }
}

pub async fn send_final_state_stream(
    server: &mut BootstrapServerBinder,
    last_key: Option<Vec<u8>>,
    final_state: Arc<RwLock<FinalState>>,
    slot: Option<Slot>,
    last_async_message_id: Option<AsyncMessageId>,
    write_timeout: Duration,
) -> Result<(), BootstrapError> {
    let mut old_key = last_key;
    let mut old_last_async_id = last_async_message_id;
    let mut old_slot = slot;

    loop {
        // Scope of the read in the final state
        let ledger_data;
        let async_pool_data;
        let final_state_changes;
        let current_slot;
        {
            // Get all data for the next message
            let final_state_read = final_state.read();
            let (data, new_last_key) =
                final_state_read
                    .ledger
                    .get_ledger_part(&old_key)
                    .map_err(|_| {
                        BootstrapError::GeneralError(
                            "Error on fetching ledger part of execution".to_string(),
                        )
                    })?;
            ledger_data = data;

            let (pool_data, last_async_pool_id) = final_state_read
                .async_pool
                .get_pool_part(old_last_async_id)?;
            async_pool_data = pool_data;

            if let Some(slot) = old_slot && let Some(key) = &old_key && let Some(async_pool_id) = old_last_async_id && slot != final_state_read.slot {
                final_state_changes = final_state_read.get_state_changes_part(
                    slot,
                    get_address_from_key(key).ok_or_else(|| BootstrapError::GeneralError("Malformed key in slot changes".to_string()))?,
                    async_pool_id,
                );
            } else {
                final_state_changes = Ok(StateChanges::default());
            }

            // Assign value for next turn
            if last_async_pool_id.is_some() || !async_pool_data.is_empty() {
                old_last_async_id = last_async_pool_id;
            }
            if new_last_key.is_some() || !ledger_data.is_empty() {
                old_key = new_last_key;
            }
            old_slot = Some(final_state_read.slot);
            current_slot = final_state_read.slot;
        }

        if !ledger_data.is_empty() || !async_pool_data.is_empty() {
            if let Ok(final_state_changes) = final_state_changes {
                match tokio::time::timeout(
                    write_timeout,
                    server.send(BootstrapServerMessage::FinalStatePart {
                        ledger_data,
                        slot: current_slot,
                        async_pool_part: async_pool_data,
                        final_state_changes,
                    }),
                )
                .await
                {
                    Err(_) => Err(std::io::Error::new(
                        std::io::ErrorKind::TimedOut,
                        "bootstrap ask ledger part send timed out",
                    )
                    .into()),
                    Ok(Err(e)) => Err(e),
                    Ok(Ok(_)) => Ok(()),
                }?;
            } else {
                match tokio::time::timeout(
                    write_timeout,
                    server.send(BootstrapServerMessage::SlotTooOld),
                )
                .await
                {
                    Err(_) => Err(std::io::Error::new(
                        std::io::ErrorKind::TimedOut,
                        "bootstrap ask ledger part send timed out",
                    )
                    .into()),
                    Ok(Err(e)) => Err(e),
                    Ok(Ok(_)) => Ok(()),
                }?;
                break;
            }
        } else {
            // There is no ledger data nor async pool data.
            match tokio::time::timeout(
                write_timeout,
                server.send(BootstrapServerMessage::FinalStateFinished),
            )
            .await
            {
                Err(_) => Err(std::io::Error::new(
                    std::io::ErrorKind::TimedOut,
                    "bootstrap ask ledger part send timed out",
                )
                .into()),
                Ok(Err(e)) => Err(e),
                Ok(Ok(_)) => Ok(()),
            }?;
            break;
        }
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
async fn manage_bootstrap(
    bootstrap_settings: &'static BootstrapSettings,
    server: &mut BootstrapServerBinder,
    data_pos: ExportProofOfStake,
    data_graph: BootstrapableGraph,
    data_peers: BootstrapPeers,
    final_state: Arc<RwLock<FinalState>>,
    compensation_millis: i64,
    version: Version,
) -> Result<(), BootstrapError> {
    massa_trace!("bootstrap.lib.manage_bootstrap", {});
    let read_error_timeout: std::time::Duration = bootstrap_settings.read_error_timeout.into();

    match tokio::time::timeout(
        bootstrap_settings.read_timeout.into(),
        server.handshake(version),
    )
    .await
    {
        Err(_) => {
            return Err(std::io::Error::new(
                std::io::ErrorKind::TimedOut,
                "bootstrap handshake send timed out",
            )
            .into())
        }
        Ok(Err(e)) => return Err(e),
        Ok(Ok(_)) => (),
    };

    match tokio::time::timeout(read_error_timeout, server.next()).await {
        Err(_) => (),
        Ok(Err(e)) => return Err(e),
        Ok(Ok(BootstrapClientMessage::BootstrapError { error })) => {
            return Err(BootstrapError::GeneralError(error))
        }
        Ok(Ok(msg)) => return Err(BootstrapError::UnexpectedClientMessage(msg)),
    };

    let write_timeout: std::time::Duration = bootstrap_settings.write_timeout.into();

    // Sync clocks.
    let server_time = MassaTime::compensated_now(compensation_millis)?;

    match tokio::time::timeout(
        write_timeout,
        server.send(BootstrapServerMessage::BootstrapTime {
            server_time,
            version,
        }),
    )
    .await
    {
        Err(_) => Err(std::io::Error::new(
            std::io::ErrorKind::TimedOut,
            "bootstrap clock send timed out",
        )
        .into()),
        Ok(Err(e)) => Err(e),
        Ok(Ok(_)) => Ok(()),
    }?;

    loop {
        match tokio::time::timeout(bootstrap_settings.read_timeout.into(), server.next()).await {
            Err(_) => return Ok(()),
            Ok(Err(e)) => return Err(e),
            Ok(Ok(msg)) => match msg {
                BootstrapClientMessage::AskBootstrapPeers => {
                    match tokio::time::timeout(
                        write_timeout,
                        server.send(BootstrapServerMessage::BootstrapPeers {
                            peers: data_peers.clone(),
                        }),
                    )
                    .await
                    {
                        Err(_) => Err(std::io::Error::new(
                            std::io::ErrorKind::TimedOut,
                            "bootstrap peers send timed out",
                        )
                        .into()),
                        Ok(Err(e)) => Err(e),
                        Ok(Ok(_)) => Ok(()),
                    }?;
                }
                BootstrapClientMessage::AskFinalStatePart {
                    last_key,
                    slot,
                    last_async_message_id,
                } => {
                    send_final_state_stream(
                        server,
                        last_key,
                        final_state.clone(),
                        slot,
                        last_async_message_id,
                        write_timeout,
                    )
                    .await?;
                }
                BootstrapClientMessage::AskConsensusState => {
                    match tokio::time::timeout(
                        write_timeout,
                        server.send(BootstrapServerMessage::ConsensusState {
                            pos: data_pos.clone(),
                            graph: data_graph.clone(),
                        }),
                    )
                    .await
                    {
                        Err(_) => Err(std::io::Error::new(
                            std::io::ErrorKind::TimedOut,
                            "bootstrap consensus state send timed out",
                        )
                        .into()),
                        Ok(Err(e)) => Err(e),
                        Ok(Ok(_)) => Ok(()),
                    }?;
                }
                BootstrapClientMessage::BootstrapSuccess => return Ok(()),
                BootstrapClientMessage::BootstrapError { error } => {
                    return Err(BootstrapError::ReceivedError(error));
                }
            },
        };
    }
}
