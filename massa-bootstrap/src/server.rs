use humantime::format_duration;
use massa_async_pool::AsyncMessageId;
use massa_consensus_exports::{bootstrapable_graph::BootstrapableGraph, ConsensusController};
use massa_final_state::{FinalState, FinalStateError};
use massa_logging::massa_trace;
use massa_models::{
    block_id::BlockId, prehash::PreHashSet, slot::Slot, streaming_step::StreamingStep,
    version::Version,
};
use massa_network_exports::NetworkCommandSender;
use massa_signature::KeyPair;
use massa_time::MassaTime;
use parking_lot::RwLock;
use std::{
    collections::{hash_map, HashMap, HashSet},
    io,
    net::{IpAddr, SocketAddr},
    path::PathBuf,
    sync::Arc,
    time::{Duration, Instant},
};
use tokio::{sync::mpsc, task::JoinHandle};
use tracing::{debug, info, warn};

use crate::{
    error::BootstrapError,
    messages::{BootstrapClientMessage, BootstrapServerMessage},
    server_binder::BootstrapServerBinder,
    tools::normalize_ip,
    BootstrapConfig, Establisher,
};

/// handle on the bootstrap server
pub struct BootstrapManager {
    join_handle: JoinHandle<Result<(), BootstrapError>>,
    manager_tx: mpsc::Sender<BSInternalMessage>,
}

enum BSInternalMessage {
    // Signals the Manager to stop
    Stop,
    // Client has completed a bootstrap
    Complete,
}

impl BootstrapManager {
    /// stop the bootstrap server
    pub async fn stop(self) -> Result<(), BootstrapError> {
        massa_trace!("bootstrap.lib.stop", {});
        if self.manager_tx.send(BSInternalMessage::Stop).await.is_err() {
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
    consensus_controller: Box<dyn ConsensusController>,
    network_command_sender: NetworkCommandSender,
    final_state: Arc<RwLock<FinalState>>,
    bootstrap_config: BootstrapConfig,
    establisher: Establisher,
    keypair: KeyPair,
    version: Version,
) -> Result<Option<BootstrapManager>, BootstrapError> {
    massa_trace!("bootstrap.lib.start_bootstrap_server", {});
    let Some(bind) = bootstrap_config.bind else {
        return Ok(None);
    };
    let (manager_tx, manager_rx) = mpsc::channel::<BSInternalMessage>(1024);

    let cloned_tx = manager_tx.clone();
    let join_handle = tokio::spawn(async move {
        BootstrapServer {
            consensus_controller,
            network_command_sender,
            final_state,
            establisher,
            manager_rx,
            manager_tx: cloned_tx,
            bind,
            keypair,
            version,
            ip_hist_map: HashMap::with_capacity(bootstrap_config.ip_list_max_size),
            bootstrap_config,
        }
        .run()
        .await
    });
    Ok(Some(BootstrapManager {
        join_handle,
        // Send on this channel to trigger the tokio::select! loop to break
        manager_tx,
    }))
}

struct BootstrapServer {
    consensus_controller: Box<dyn ConsensusController>,
    network_command_sender: NetworkCommandSender,
    final_state: Arc<RwLock<FinalState>>,
    establisher: Establisher,
    manager_rx: mpsc::Receiver<BSInternalMessage>,
    manager_tx: mpsc::Sender<BSInternalMessage>,
    bind: SocketAddr,
    keypair: KeyPair,
    bootstrap_config: BootstrapConfig,
    version: Version,
    ip_hist_map: HashMap<IpAddr, Instant>,
}

#[allow(clippy::result_large_err)]
#[allow(clippy::type_complexity)]
fn reload_whitelist_blacklist(
    whitelist_path: &PathBuf,
    blacklist_path: &PathBuf,
) -> Result<(Option<HashSet<IpAddr>>, Option<HashSet<IpAddr>>), BootstrapError> {
    let whitelist = if let Ok(whitelist) = std::fs::read_to_string(whitelist_path) {
        Some(
            serde_json::from_str::<HashSet<IpAddr>>(whitelist.as_str())
                .map_err(|_| {
                    BootstrapError::GeneralError(String::from(
                        "Failed to parse bootstrap whitelist",
                    ))
                })?
                .into_iter()
                .map(normalize_ip)
                .collect(),
        )
    } else {
        None
    };

    let blacklist = if let Ok(blacklist) = std::fs::read_to_string(blacklist_path) {
        Some(
            serde_json::from_str::<HashSet<IpAddr>>(blacklist.as_str())
                .map_err(|_| {
                    BootstrapError::GeneralError(String::from(
                        "Failed to parse bootstrap blacklist",
                    ))
                })?
                .into_iter()
                .map(normalize_ip)
                .collect(),
        )
    } else {
        None
    };
    Ok((whitelist, blacklist))
}

impl BootstrapServer {
    pub async fn run(mut self) -> Result<(), BootstrapError> {
        debug!("starting bootstrap server");
        massa_trace!("bootstrap.lib.run", {});
        let mut listener = self.establisher.get_listener(self.bind).await?;
        // let mut bootstrap_sessions = FuturesUnordered::new();
        let cache_timeout = self.bootstrap_config.cache_duration.to_duration();
        let (mut whitelist, mut blacklist) = reload_whitelist_blacklist(
            &self.bootstrap_config.bootstrap_whitelist_path,
            &self.bootstrap_config.bootstrap_blacklist_path,
        )?;
        let mut cache_interval = tokio::time::interval(cache_timeout);
        /*
            select! without the "biased" modifier will randomly select the 1st branch to check,
            then will check the next ones in the order they are written.
            We choose this order:
                * manager commands to avoid waiting too long to stop in case of contention
                * cache timeout to avoid skipping timeouts cleanup tasks (they are relatively rare)
                * bootstrap sessions (rare)
                * listener: most frequent => last
        */

        // Use the strong-count of this variable to track the session count
        let bootstrap_sessions_counter: Arc<()> = Arc::new(());
        loop {
            massa_trace!("bootstrap.lib.run.select", {});
            tokio::select! {
                msg = self.manager_rx.recv() => {
                    let Some(msg) = msg else {
                        return Err(BootstrapError::GeneralError("command channel closed prematurely".into()));
                    };
                    match  msg {
                        BSInternalMessage::Stop => {
                            massa_trace!("bootstrap.lib.run.select.manager", {});
                            break
                        },
                        BSInternalMessage::Complete => {
                            massa_trace!("bootstrap.session.finished", {"active_count": Arc::strong_count(&bootstrap_sessions_counter) - 1});
                        }
                    }
                }
                _ = cache_interval.tick() => {
                    (whitelist, blacklist) = reload_whitelist_blacklist(&self.bootstrap_config.bootstrap_whitelist_path, &self.bootstrap_config.bootstrap_blacklist_path)?;
                }

                // listener
                // Potential issue: because it's async, a connection match could be cancelled before spawning
                // the actual bootstrapping. Cancellation would occur through the Stop command going through
                Ok((dplx, remote_addr)) = listener.accept() => {
                    // claim a slot in the max_bootstrap_sessions
                    let bootstrap_count_token = bootstrap_sessions_counter.clone();
                    let server = BootstrapServerBinder::new(
                        dplx,
                        self.keypair.clone(),
                        self.bootstrap_config.max_bytes_read_write,
                        self.bootstrap_config.max_bootstrap_message_size,
                        self.bootstrap_config.thread_count,
                        self.bootstrap_config.max_datastore_key_length,
                        self.bootstrap_config.randomness_size_bytes,
                        self.bootstrap_config.consensus_bootstrap_part_size
                    );

                    // check whether incoming peer IP is allowed or return an error which is ignored
                    let Ok((mut server, remote_addr)) = self.is_ip_allowed(remote_addr, server, &whitelist, &blacklist).await else {
                        continue;
                    };

                    let max_bootstraps: usize = self.bootstrap_config.max_simultaneous_bootstraps.try_into().map_err(|_| BootstrapError::GeneralError("Fail to convert u32 to usize".to_string()))?;
                    // TODO: double-check OBO errors here, or find a better way to track count
                    if Arc::strong_count(&bootstrap_sessions_counter) < max_bootstraps {

                        let per_ip_min_interval = self.bootstrap_config.per_ip_min_interval.to_duration();
                        massa_trace!("bootstrap.lib.run.select.accept", {"remote_addr": remote_addr});
                        let now = Instant::now();

                        // clear IP history if necessary
                        if self.ip_hist_map.len() > self.bootstrap_config.ip_list_max_size {
                            self.ip_hist_map.retain(|_k, v| now.duration_since(*v) <= per_ip_min_interval);
                            if self.ip_hist_map.len() > self.bootstrap_config.ip_list_max_size {
                                // too many IPs are spamming us: clear cache
                                warn!("high bootstrap load: at least {} different IPs attempted bootstrap in the last {}", self.ip_hist_map.len(),format_duration(self.bootstrap_config.per_ip_min_interval.to_duration()).to_string());
                                self.ip_hist_map.clear();
                            }
                        }

                        // check IP's bootstrap attempt history
                        let Ok(binding) =  BootstrapServer::bootstrap_client_check(server, &mut self.ip_hist_map, self.bootstrap_config.write_error_timeout.into(), remote_addr, now, per_ip_min_interval).await else {
                            continue;
                        };


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

                        let version = self.version;
                        let data_execution = self.final_state.clone();
                        let consensus_command_sender = self.consensus_controller.clone();
                        let network_command_sender = self.network_command_sender.clone();
                        let config = self.bootstrap_config.clone();

                        tokio::spawn(run_bootstrap_session(
                            binding,
                            self.manager_tx.clone(),
                            bootstrap_count_token,
                            config,
                            remote_addr,
                            data_execution,
                            version,
                            consensus_command_sender,
                            network_command_sender
                        ));
                        // increment the number of active bootstraps. Will be decremented when handling the
                        // Complete message that is sent when complete.

                        massa_trace!("bootstrap.session.started", {"active_count": Arc::strong_count(&bootstrap_sessions_counter) - 1});
                    } else {
                        let config = self.bootstrap_config.clone();
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
        }

        Ok(())
    }

    /// check IP's bootstrap attempt history, consuming the server if fail, giving it back for later use otherwise.
    async fn bootstrap_client_check(
        mut server: BootstrapServerBinder,
        ip_hist_map: &mut HashMap<IpAddr, Instant>,
        error_to: Duration,
        remote_addr: SocketAddr,
        now: Instant,
        per_ip_min_interval: Duration,
    ) -> Result<BootstrapServerBinder, ()> {
        match ip_hist_map.entry(remote_addr.ip()) {
            hash_map::Entry::Occupied(mut occ) => {
                if now.duration_since(*occ.get()) <= per_ip_min_interval {
                    let send_timeout = tokio::time::timeout(
                                        error_to,
                                        server.send(
                                            BootstrapServerMessage::BootstrapError {
                                                error: format!(
                                                    "Your last bootstrap on this server was {} ago and you have to wait {} before retrying.",
                                                    format_duration(occ.get().elapsed()),
                                                    format_duration(per_ip_min_interval.saturating_sub(occ.get().elapsed()))
                                                )
                                            }
                                        )).await;

                    let _ = match send_timeout {
                        Err(_) => Err(std::io::Error::new(
                            std::io::ErrorKind::TimedOut,
                            "bootstrap error too early retry bootstrap send timed out",
                        )
                        .into()),
                        Ok(Err(e)) => Err(e),
                        Ok(Ok(_)) => Ok(()),
                    };
                    // in list, non-expired => refuse
                    massa_trace!("bootstrap.lib.run.select.accept.refuse_limit", {
                        "remote_addr": remote_addr
                    });
                    return Err(());
                } else {
                    // in list, expired
                    occ.insert(now);
                }
            }
            hash_map::Entry::Vacant(vac) => {
                vac.insert(now);
            }
        }
        Ok(server)
    }

    #[cfg(test)]
    // TODO we didn't test whether the peer IP address is banned
    async fn is_ip_allowed(
        &self,
        remote_addr: SocketAddr,
        server: BootstrapServerBinder,
        _whitelist: &Option<HashSet<IpAddr>>,
        _blacklist: &Option<HashSet<IpAddr>>,
    ) -> io::Result<(BootstrapServerBinder, SocketAddr)> {
        Ok((server, remote_addr))
    }

    #[cfg(not(test))]
    // whether the peer IP address is banned
    async fn is_ip_allowed(
        &self,
        remote_addr: SocketAddr,
        mut server: BootstrapServerBinder,
        whitelist: &Option<HashSet<IpAddr>>,
        blacklist: &Option<HashSet<IpAddr>>,
    ) -> io::Result<(BootstrapServerBinder, SocketAddr)> {
        let ip = normalize_ip(remote_addr.ip());
        // whether the peer IP address is blacklisted
        let not_allowed_msg = if let Some(ip_list) = &blacklist && ip_list.contains(&ip) {
            massa_trace!("bootstrap.lib.run.select.accept.refuse_blacklisted", {"remote_addr": remote_addr});
            Some(format!("IP {} is blacklisted", &ip))
        // whether the peer IP address is not present in the whitelist
        } else if let Some(ip_list) = &whitelist && !ip_list.contains(&ip){
            massa_trace!("bootstrap.lib.run.select.accept.refuse_not_whitelisted", {"remote_addr": remote_addr});
            Some(format!("A whitelist exists and the IP {} is not whitelisted", &ip))
        } else {
            None
        };

        // whether the peer IP address is not allowed, send back an error message
        if let Some(error_msg) = not_allowed_msg {
            let _ = match tokio::time::timeout(
                self.bootstrap_config.write_error_timeout.into(),
                server.send(BootstrapServerMessage::BootstrapError {
                    error: error_msg.clone(),
                }),
            )
            .await
            {
                Err(_) => Err(std::io::Error::new(
                    std::io::ErrorKind::PermissionDenied,
                    format!("{}  timed out", &error_msg),
                )
                .into()),
                Ok(Err(e)) => Err(e),
                Ok(Ok(_)) => Ok(()),
            };
            return Err(std::io::Error::new(
                std::io::ErrorKind::PermissionDenied,
                error_msg,
            ));
        }

        Ok((server, remote_addr))
    }
}

/// TODO: Doc-comment me
#[allow(clippy::too_many_arguments)]
async fn run_bootstrap_session(
    mut server: BootstrapServerBinder,
    controller_tx: mpsc::Sender<BSInternalMessage>,
    arc_counter: Arc<()>,
    config: BootstrapConfig,
    remote_addr: SocketAddr,
    data_execution: Arc<RwLock<FinalState>>,
    version: Version,
    consensus_command_sender: Box<dyn ConsensusController>,
    network_command_sender: NetworkCommandSender,
) {
    debug!("awaiting on bootstrap of peer {}", remote_addr);
    let res = tokio::time::timeout(
        config.bootstrap_timeout.into(),
        manage_bootstrap(
            &config,
            &mut server,
            data_execution,
            version,
            consensus_command_sender,
            network_command_sender,
        ),
    )
    .await;
    // This drop allows the server to accept new connections without having to complete the error notifications
    drop(arc_counter);
    let _ = controller_tx.send(BSInternalMessage::Complete).await;
    match res {
        Ok(mgmt) => match mgmt {
            Ok(_) => {
                info!("bootstrapped peer {}", remote_addr)
            }
            Err(BootstrapError::ReceivedError(error)) => debug!(
                "bootstrap serving error received from peer {}: {}",
                remote_addr, error
            ),
            Err(err) => {
                debug!("bootstrap serving error for peer {}: {}", remote_addr, err);
                // We allow unused result because we don't care if an error is thrown when
                // sending the error message to the server we will close the socket anyway.
                let _ = tokio::time::timeout(
                    config.write_error_timeout.into(),
                    server.send(BootstrapServerMessage::BootstrapError {
                        error: err.to_string(),
                    }),
                )
                .await;
            }
        },
        Err(_timeout) => {
            debug!("bootstrap timeout for peer {}", remote_addr);
            // We allow unused result because we don't care if an error is thrown when
            // sending the error message to the server we will close the socket anyway.
            let _ = tokio::time::timeout(
                config.write_error_timeout.into(),
                server.send(BootstrapServerMessage::BootstrapError {
                    error: format!(
                        "Bootstrap process timedout ({})",
                        format_duration(config.bootstrap_timeout.to_duration())
                    ),
                }),
            )
            .await;
        }
    }
}

#[allow(clippy::too_many_arguments)]
pub async fn stream_bootstrap_information(
    server: &mut BootstrapServerBinder,
    final_state: Arc<RwLock<FinalState>>,
    consensus_controller: Box<dyn ConsensusController>,
    mut last_slot: Option<Slot>,
    mut last_ledger_step: StreamingStep<Vec<u8>>,
    mut last_pool_step: StreamingStep<AsyncMessageId>,
    mut last_cycle_step: StreamingStep<u64>,
    mut last_credits_step: StreamingStep<Slot>,
    mut last_ops_step: StreamingStep<Slot>,
    mut last_consensus_step: StreamingStep<PreHashSet<BlockId>>,
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

        let mut slot_too_old = false;

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
                final_state_changes = match final_state_read.get_state_changes_part(
                    slot,
                    new_ledger_step.clone(),
                    new_pool_step,
                    new_cycle_step,
                    new_credits_step,
                    new_ops_step,
                ) {
                    Ok(data) => data,
                    Err(err) if matches!(err, FinalStateError::InvalidSlot(_)) => {
                        slot_too_old = true;
                        Vec::default()
                    }
                    Err(err) => return Err(BootstrapError::FinalStateError(err)),
                };
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

        if slot_too_old {
            match tokio::time::timeout(
                write_timeout,
                server.send(BootstrapServerMessage::SlotTooOld),
            )
            .await
            {
                Err(_) => Err(std::io::Error::new(
                    std::io::ErrorKind::TimedOut,
                    "SlotTooOld message send timed out",
                )
                .into()),
                Ok(Err(e)) => Err(e),
                Ok(Ok(_)) => Ok(()),
            }?;
            return Ok(());
        }

        // Setup final state global cursor
        let final_state_global_step = if last_ledger_step.finished()
            && last_pool_step.finished()
            && last_cycle_step.finished()
            && last_credits_step.finished()
            && last_ops_step.finished()
        {
            StreamingStep::Finished(Some(current_slot))
        } else {
            StreamingStep::Ongoing(current_slot)
        };

        // Setup final state changes cursor
        let final_state_changes_step = if final_state_changes.is_empty() {
            StreamingStep::Finished(Some(current_slot))
        } else {
            StreamingStep::Ongoing(current_slot)
        };

        // Stream consensus blocks if final state base bootstrap is finished
        let mut consensus_part = BootstrapableGraph {
            final_blocks: Default::default(),
        };
        let mut consensus_outdated_ids: PreHashSet<BlockId> = PreHashSet::default();
        if final_state_global_step.finished() {
            let (part, outdated_ids, new_consensus_step) = consensus_controller
                .get_bootstrap_part(last_consensus_step, final_state_changes_step)?;
            consensus_part = part;
            consensus_outdated_ids = outdated_ids;
            last_consensus_step = new_consensus_step;
        }

        // Logs for an easier diagnostic if needed
        debug!(
            "Final state bootstrap cursor: {:?}",
            final_state_global_step
        );
        debug!(
            "Consensus blocks bootstrap cursor: {:?}",
            last_consensus_step
        );
        if let StreamingStep::Ongoing(ids) = &last_consensus_step {
            debug!("Consensus bootstrap cursor length: {}", ids.len());
        }

        // If the consensus streaming is finished (also meaning that consensus slot == final state slot) exit
        if final_state_global_step.finished()
            && final_state_changes_step.finished()
            && last_consensus_step.finished()
        {
            match tokio::time::timeout(
                write_timeout,
                server.send(BootstrapServerMessage::BootstrapFinished),
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

        // At this point we know that consensus, final state or both are not finished
        match tokio::time::timeout(
            write_timeout,
            server.send(BootstrapServerMessage::BootstrapPart {
                slot: current_slot,
                ledger_part,
                async_pool_part,
                pos_cycle_part,
                pos_credits_part,
                exec_ops_part,
                final_state_changes,
                consensus_part,
                consensus_outdated_ids,
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
    }
    Ok(())
}

#[allow(clippy::manual_async_fn)]
#[allow(clippy::too_many_arguments)]
async fn manage_bootstrap(
    bootstrap_config: &BootstrapConfig,
    server: &mut BootstrapServerBinder,
    final_state: Arc<RwLock<FinalState>>,
    version: Version,
    consensus_controller: Box<dyn ConsensusController>,
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
    let server_time = MassaTime::now()?;

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
                BootstrapClientMessage::AskBootstrapPart {
                    last_slot,
                    last_ledger_step,
                    last_pool_step,
                    last_cycle_step,
                    last_credits_step,
                    last_ops_step,
                    last_consensus_step,
                } => {
                    stream_bootstrap_information(
                        server,
                        final_state.clone(),
                        consensus_controller.clone(),
                        last_slot,
                        last_ledger_step,
                        last_pool_step,
                        last_cycle_step,
                        last_credits_step,
                        last_ops_step,
                        last_consensus_step,
                        write_timeout,
                    )
                    .await?;
                }
                BootstrapClientMessage::BootstrapSuccess => break Ok(()),
                BootstrapClientMessage::BootstrapError { error } => {
                    break Err(BootstrapError::ReceivedError(error));
                }
            },
        };
    }
}
