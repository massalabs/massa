use std::{
    collections::{hash_map, HashMap, HashSet},
    net::{IpAddr, SocketAddr},
    sync::Arc,
    time::{Duration, Instant},
};

use futures::stream::FuturesUnordered;
use futures::StreamExt;
use massa_async_pool::AsyncMessageId;
use massa_consensus_exports::ConsensusCommandSender;
use massa_final_state::FinalState;
use massa_logging::massa_trace;
use massa_models::{
    operation::OperationId, slot::Slot, streaming_step::StreamingStep, version::Version,
};
use massa_network_exports::NetworkCommandSender;
use massa_signature::KeyPair;
use massa_time::MassaTime;
use parking_lot::RwLock;
use tokio::{sync::mpsc, task::JoinHandle};
use tracing::{debug, info, warn};

use crate::{
    error::BootstrapError,
    messages::{BootstrapClientMessage, BootstrapServerMessage},
    server_binder::BootstrapServerBinder,
    BootstrapConfig, Establisher,
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
    bootstrap_config: BootstrapConfig,
    establisher: Establisher,
    keypair: KeyPair,
    compensation_millis: i64,
    version: Version,
) -> Result<Option<BootstrapManager>, BootstrapError> {
    massa_trace!("bootstrap.lib.start_bootstrap_server", {});
    if let Some(bind) = bootstrap_config.bind {
        let (manager_tx, manager_rx) = mpsc::channel::<()>(1);
        let whitelist = serde_json::from_str::<HashSet<IpAddr>>(
            &std::fs::read_to_string(&bootstrap_config.bootstrap_whitelist_file)?).map_err(|_| BootstrapError::GeneralError(String::from("Failed to parse bootstrap whitelist")))?
            .into_iter()
            .map(|ip|  ip.to_canonical()).collect();
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
                whitelist,
                ip_hist_map: HashMap::with_capacity(bootstrap_config.ip_list_max_size),
                bootstrap_config,
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
    bootstrap_config: BootstrapConfig,
    compensation_millis: i64,
    version: Version,
    whitelist: HashSet<IpAddr>,
    ip_hist_map: HashMap<IpAddr, Instant>,
}

impl BootstrapServer {
    pub async fn run(mut self) -> Result<(), BootstrapError> {
        debug!("starting bootstrap server");
        massa_trace!("bootstrap.lib.run", {});
        let mut listener = self.establisher.get_listener(self.bind).await?;
        let mut bootstrap_sessions = FuturesUnordered::new();
        // let cache_timeout = self.bootstrap_config.cache_duration.to_duration();
        // let mut bootstrap_data: Option<(
        //     BootstrapableGraph,
        //     BootstrapPeers,
        //     Arc<RwLock<FinalState>>,
        // )> = None;
        // let cache_timer = sleep(cache_timeout);
        let per_ip_min_interval = self.bootstrap_config.per_ip_min_interval.to_duration();
        // tokio::pin!(cache_timer);
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
                // _ = &mut cache_timer, if bootstrap_data.is_some() => {
                //     massa_trace!("bootstrap.lib.run.cache_unload", {});
                //     bootstrap_data = None;
                // }

                // bootstrap session finished
                Some(_) = bootstrap_sessions.next() => {
                    massa_trace!("bootstrap.session.finished", {"active_count": bootstrap_sessions.len()});
                }

                // listener
                Ok((dplx, remote_addr)) = listener.accept() => 
                if (bootstrap_sessions.len() < self.bootstrap_config.max_simultaneous_bootstraps.try_into().map_err(|_| BootstrapError::GeneralError("Fail to convert u32 to usize".to_string()))?) || self.whitelist.contains(&remote_addr.ip().to_canonical()) {

                    massa_trace!("bootstrap.lib.run.select.accept", {"remote_addr": remote_addr});
                    let now = Instant::now();

                    // clear IP history if necessary
                    if self.ip_hist_map.len() > self.bootstrap_config.ip_list_max_size {
                        self.ip_hist_map.retain(|_k, v| now.duration_since(*v) <= per_ip_min_interval);
                        if self.ip_hist_map.len() > self.bootstrap_config.ip_list_max_size {
                            // too many IPs are spamming us: clear cache
                            warn!("high bootstrap load: at least {} different IPs attempted bootstrap in the last {}ms", self.ip_hist_map.len(), self.bootstrap_config.per_ip_min_interval);
                            self.ip_hist_map.clear();
                        }
                    }

                    // check IP's bootstrap attempt history
                    match self.ip_hist_map.entry(remote_addr.ip()) {
                        hash_map::Entry::Occupied(mut occ) => {
                            if now.duration_since(*occ.get()) <= per_ip_min_interval && !self.whitelist.contains(&remote_addr.ip().to_canonical()) {
                                let mut server = BootstrapServerBinder::new(dplx, self.keypair.clone(), self.bootstrap_config.max_bytes_read_write, self.bootstrap_config.max_bootstrap_message_size, self.bootstrap_config.thread_count, self.bootstrap_config.max_datastore_key_length, self.bootstrap_config.randomness_size_bytes);
                                let _ = match tokio::time::timeout(self.bootstrap_config.write_error_timeout.into(), server.send(BootstrapServerMessage::BootstrapError {
                                    error:
                                    format!("Your last bootstrap on this server was {:#?} ago and you have to wait {:#?} before retrying.", occ.get().elapsed(), per_ip_min_interval.saturating_sub(occ.get().elapsed()))
                                })).await {
                                    Err(_) => Err(std::io::Error::new(std::io::ErrorKind::TimedOut, "bootstrap error too early retry bootstrap send timed out").into()),
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
                    // if bootstrap_data.is_none() {
                    //     massa_trace!("bootstrap.lib.run.select.accept.cache_load.start", {});

                    //     // Note that all requests are done simultaneously except for the consensus graph that is done after the others.
                    //     // This is done to ensure that the execution bootstrap state is older than the consensus state.
                    //     // If the consensus state snapshot is older than the execution state snapshot,
                    //     //   the execution final ledger will be in the future after bootstrap, which causes an inconsistency.
                    //     bootstrap_data = Some((data_graph, data_peers, self.final_state.clone()));
                    //     cache_timer.set(sleep(cache_timeout));
                    // }
                    massa_trace!("bootstrap.lib.run.select.accept.cache_available", {});

                    // launch bootstrap

                    let compensation_millis = self.compensation_millis;
                    let version = self.version;
                    let data_execution = self.final_state.clone();
                    let consensus_command_sender = self.consensus_command_sender.clone();
                    let network_command_sender = self.network_command_sender.clone();
                    let keypair = self.keypair.clone();
                    let config = self.bootstrap_config.clone();

                    bootstrap_sessions.push(async move {
                        let mut server = BootstrapServerBinder::new(dplx, keypair, config.max_bytes_read_write, config.max_bootstrap_message_size, config.thread_count, config.max_datastore_key_length, config.randomness_size_bytes);
                        match manage_bootstrap(&config, &mut server, data_execution, compensation_millis, version, consensus_command_sender, network_command_sender).await {
                            Ok(_) => {
                                info!("bootstrapped peer {}", remote_addr)
                            },
                            Err(BootstrapError::ReceivedError(error)) => debug!("bootstrap serving error received from peer {}: {}", remote_addr, error),
                            Err(err) => {
                                debug!("bootstrap serving error for peer {}: {}", remote_addr, err);
                                // We allow unused result because we don't care if an error is thrown when sending the error message to the server we will close the socket anyway.
                                let _ = tokio::time::timeout(config.write_error_timeout.into(), server.send(BootstrapServerMessage::BootstrapError { error: err.to_string() })).await;
                            },
                        }

                    });
                    massa_trace!("bootstrap.session.started", {"active_count": bootstrap_sessions.len()});
                } else {
                    let config = self.bootstrap_config.clone();
                    let mut server = BootstrapServerBinder::new(dplx, self.keypair.clone(), config.max_bytes_read_write, config.max_bootstrap_message_size, config.thread_count, config.max_datastore_key_length, config.randomness_size_bytes);
                    let _ = match tokio::time::timeout(config.clone().write_error_timeout.into(), server.send(BootstrapServerMessage::BootstrapError {
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

#[allow(clippy::too_many_arguments)]
pub async fn send_final_state_stream(
    server: &mut BootstrapServerBinder,
    final_state: Arc<RwLock<FinalState>>,
    mut last_slot: Option<Slot>,
    mut last_ledger_step: StreamingStep<Vec<u8>>,
    mut last_pool_step: StreamingStep<AsyncMessageId>,
    mut last_cycle_step: StreamingStep<u64>,
    mut last_credits_step: StreamingStep<Slot>,
    mut last_ops_step: StreamingStep<OperationId>,
    write_timeout: Duration,
) -> Result<(), BootstrapError> {
    loop {
        #[cfg(test)]
        {
            // Necessary for test_bootstrap_server in tests/scenarios.rs
            tokio::time::sleep(Duration::from_millis(500)).await;
        }

        let current_slot;
        let ledger_part;
        let async_pool_part;
        let pos_cycle_part;
        let pos_credits_part;
        let exec_ops_part;
        let final_state_changes;

        // Scope of the final state read
        {
            let final_state_read = final_state.read();
            let (data, new_ledger_step) = final_state_read
                .ledger
                .get_ledger_part(last_ledger_step.clone())?;
            ledger_part = data;

            let (pool_data, new_pool_step) =
                final_state_read.async_pool.get_pool_part(last_pool_step);
            async_pool_part = pool_data;

            let (cycle_data, new_cycle_step) = final_state_read
                .pos_state
                .get_cycle_history_part(last_cycle_step)?;
            pos_cycle_part = cycle_data;

            let (credits_data, new_credits_step) = final_state_read
                .pos_state
                .get_deferred_credits_part(last_credits_step);
            pos_credits_part = credits_data;

            let (ops_data, new_ops_step) = final_state_read
                .executed_ops
                .get_executed_ops_part(last_ops_step);
            exec_ops_part = ops_data;

            if let Some(slot) = last_slot && slot != final_state_read.slot {
                if slot > final_state_read.slot {
                    return Err(BootstrapError::GeneralError(
                        "Bootstrap cursor set to future slot".to_string(),
                    ));
                }
                final_state_changes = final_state_read.get_state_changes_part(
                    slot,
                    new_ledger_step.clone(),
                    new_pool_step,
                    new_cycle_step,
                    new_credits_step,
                    new_ops_step,
                )?;
            } else {
                final_state_changes = Vec::new();
            }

            // Update cursors for next turn
            last_ledger_step = new_ledger_step;
            last_pool_step = new_pool_step;
            last_cycle_step = new_cycle_step;
            last_credits_step = new_credits_step;
            last_ops_step = new_ops_step;
            last_slot = Some(final_state_read.slot);
            current_slot = final_state_read.slot;
        }

        if !last_ledger_step.finished()
            || !last_pool_step.finished()
            || !last_cycle_step.finished()
            || !last_credits_step.finished()
            || !last_ops_step.finished()
            || !final_state_changes.is_empty()
        {
            match tokio::time::timeout(
                write_timeout,
                server.send(BootstrapServerMessage::FinalStatePart {
                    slot: current_slot,
                    ledger_part,
                    async_pool_part,
                    pos_cycle_part,
                    pos_credits_part,
                    exec_ops_part,
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
    bootstrap_config: &BootstrapConfig,
    server: &mut BootstrapServerBinder,
    final_state: Arc<RwLock<FinalState>>,
    compensation_millis: i64,
    version: Version,
    consensus_command_sender: ConsensusCommandSender,
    network_command_sender: NetworkCommandSender,
) -> Result<(), BootstrapError> {
    massa_trace!("bootstrap.lib.manage_bootstrap", {});
    let read_error_timeout: std::time::Duration = bootstrap_config.read_error_timeout.into();

    match tokio::time::timeout(
        bootstrap_config.read_timeout.into(),
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

    let write_timeout: std::time::Duration = bootstrap_config.write_timeout.into();

    // Sync clocks.
    let server_time = MassaTime::now(compensation_millis)?;

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
        match tokio::time::timeout(bootstrap_config.read_timeout.into(), server.next()).await {
            Err(_) => break Ok(()),
            Ok(Err(e)) => break Err(e),
            Ok(Ok(msg)) => match msg {
                BootstrapClientMessage::AskBootstrapPeers => {
                    match tokio::time::timeout(
                        write_timeout,
                        server.send(BootstrapServerMessage::BootstrapPeers {
                            peers: network_command_sender.get_bootstrap_peers().await?,
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
                    last_slot,
                    last_ledger_step,
                    last_pool_step,
                    last_cycle_step,
                    last_credits_step,
                    last_ops_step,
                } => {
                    send_final_state_stream(
                        server,
                        final_state.clone(),
                        last_slot,
                        last_ledger_step,
                        last_pool_step,
                        last_cycle_step,
                        last_credits_step,
                        last_ops_step,
                        write_timeout,
                    )
                    .await?;
                }
                BootstrapClientMessage::AskConsensusState => {
                    match tokio::time::timeout(
                        write_timeout,
                        server.send(BootstrapServerMessage::ConsensusState {
                            graph: consensus_command_sender.get_bootstrap_state().await?,
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
                BootstrapClientMessage::BootstrapSuccess => break Ok(()),
                BootstrapClientMessage::BootstrapError { error } => {
                    break Err(BootstrapError::ReceivedError(error));
                }
            },
        };
    }
}
