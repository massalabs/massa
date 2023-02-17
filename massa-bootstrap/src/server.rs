mod allow_block_list;
use allow_block_list::*;

use crossbeam::channel::Receiver;
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
    collections::HashMap,
    net::{IpAddr, SocketAddr},
    sync::Arc,
    time::{Duration, Instant},
};
use tokio::runtime::Runtime;
use tracing::{debug, info, warn};

use crate::{
    error::BootstrapError,
    messages::{BootstrapClientMessage, BootstrapServerMessage},
    server_binder::BootstrapServerBinder,
    types::Duplex,
    BootstrapConfig, Establisher,
};

/// handle on the bootstrap server
pub struct BootstrapManager {
    join_handle: std::thread::JoinHandle<Result<(), BootstrapError>>,
    manager_tx: crossbeam::channel::Sender<()>,
}

impl BootstrapManager {
    /// stop the bootstrap server
    pub fn stop(self) -> Result<(), BootstrapError> {
        massa_trace!("bootstrap.lib.stop", {});
        if self.manager_tx.send(()).is_err() {
            warn!("bootstrap server already dropped");
        }
        self.join_handle.join().unwrap()
    }
}

/// start a bootstrap server.
/// Once your node will be ready, you may want other to bootstrap from you.
pub fn start_bootstrap_server(
    consensus_controller: Box<dyn ConsensusController>,
    network_command_sender: NetworkCommandSender,
    final_state: Arc<RwLock<FinalState>>,
    config: BootstrapConfig,
    establisher: Establisher,
    keypair: KeyPair,
    version: Version,
) -> Result<Option<BootstrapManager>, BootstrapError> {
    massa_trace!("bootstrap.lib.start_bootstrap_server", {});
    let Some(listen_addr) = config.listen_addr else {
        return Ok(None);
    };
    // zero-capacity channel is being tried. The idea being is that the receiver can then be a dedicated
    // thread that will only be activated when the stop signal is finally activated
    let (manager_tx, manager_rx) = crossbeam::channel::bounded::<()>(0);

    let runtime = Runtime::new().expect("Failed to create a bootstrap runtime");

    let join_handle = std::thread::spawn(move || {
        BootstrapServer {
            consensus_controller,
            network_command_sender,
            final_state,
            establisher,
            manager_rx,
            bind: listen_addr,
            keypair,
            version,
            ip_hist_map: HashMap::with_capacity(config.ip_list_max_size),
            bootstrap_config: config,
            runtime,
        }
        .run()
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
    manager_rx: Receiver<()>,
    bind: SocketAddr,
    keypair: KeyPair,
    bootstrap_config: BootstrapConfig,
    version: Version,
    ip_hist_map: HashMap<IpAddr, Instant>,
    runtime: Runtime,
}

impl BootstrapServer {
    pub fn run(mut self) -> Result<(), BootstrapError> {
        debug!("starting bootstrap server");
        massa_trace!("bootstrap.lib.run", {});
        let mut listener = self
            .runtime
            .block_on(self.establisher.get_listener(self.bind))?;
        let list_update_timeout = self.bootstrap_config.cache_duration.to_duration();

        let per_ip_min_interval = self.bootstrap_config.per_ip_min_interval.to_duration();

        let Ok(max_bootstraps) = self.bootstrap_config.max_simultaneous_bootstraps.try_into() else {
            return Err(BootstrapError::GeneralError("Fail to convert u32 to usize".to_string()));
        };

        // This is the primary interface between the async-listener, and the (soon to be) sync worker
        let (listener_tx, listener_rx) =
            crossbeam::channel::bounded::<(Duplex, SocketAddr)>(max_bootstraps);

        // As far as the bootstrap thread is concerned, the lists are auto-magically kept upt to date
        // This is done by providing an opaque interface, behind which, is an Arc<RwLock<lists>> data
        // that is thread-safe updated with minimal contention by the update method.
        // The cost of this, is when an update is run, the thread does expensive file read and
        // deserialization ops.
        //
        // Ideally, we would cache a set of add/remove to the two lists, and periodically
        // check and apply the changes.
        let allow_block_list = SharedAllowBlockList::new(
            self.bootstrap_config.bootstrap_whitelist_path.clone(),
            self.bootstrap_config.bootstrap_blacklist_path.clone(),
        )
        .map_err(BootstrapError::GeneralError)?;
        let mut updaters_list = allow_block_list.clone();
        std::thread::spawn(move || loop {
            std::thread::sleep(list_update_timeout);
            // TODO: Do we want to handle shutting things down if the update errors out?
            let _ = updaters_list.update();
        });

        let listener_handle = self.runtime.spawn(async move {
            // todo!("it seems this context doesn't run, causing everything to lock-up");
            loop {
                // This needs to become sync somehow? the blocking send might include other async runtimes otherwise...
                let Ok((dplx, remote_addr)) = listener.accept().await else {
                    continue;
                };
                match listener_tx.send((dplx, remote_addr)) {
                    Ok(_) => {}
                    Err(e) => todo!("handle closed channel after accept: {:?}", e.into_inner().1),
                }
            }
        });

        let loop_handle = std::thread::spawn(move || {
            self.run_loop(
                listener_rx,
                allow_block_list,
                max_bootstraps,
                per_ip_min_interval,
            )
        });
        let _ = loop_handle.join().unwrap();
        listener_handle.abort();

        Ok(())
    }

    fn run_loop(
        &mut self,
        listener_rx: Receiver<(Duplex, SocketAddr)>,
        allow_block_list: SharedAllowBlockList,
        max_bootstraps: usize,
        per_ip_min_interval: Duration,
    ) {
        // Use the strong-count of this variable to track the session count
        let bootstrap_sessions_counter: Arc<()> = Arc::new(());
        loop {
            massa_trace!("bootstrap.lib.run.select", {});
            // before handling a bootstrap, check if the stopper has sent a trigger.
            // TODO: There is probably a better way to do this, such as using an Arc<AtomicBool>...
            let stop = self.manager_rx.try_recv();
            // give the compiler optimisation hints
            if unlikely(stop.is_ok()) {
                massa_trace!("bootstrap.lib.run.select.manager", {});
                break;
            } else if unlikely(stop == Err(crossbeam::channel::TryRecvError::Disconnected)) {
                todo!("handle closed stopper")
            };

            // listener
            let  Ok((dplx, remote_addr)) = listener_rx.recv() else {continue;};

            // claim a slot in the max_bootstrap_sessions
            let bootstrap_count_token = bootstrap_sessions_counter.clone();
            let mut server = BootstrapServerBinder::new(
                dplx,
                self.keypair.clone(),
                self.bootstrap_config.max_bytes_read_write,
                self.bootstrap_config.max_bootstrap_message_size,
                self.bootstrap_config.thread_count,
                self.bootstrap_config.max_datastore_key_length,
                self.bootstrap_config.randomness_size_bytes,
                self.bootstrap_config.consensus_bootstrap_part_size,
                self.bootstrap_config.write_error_timeout,
            );

            // check whether incoming peer IP is allowed.
            // TODO: confirm error handled according to the previous `is_ip_allowed` fn
            if let Err(error_msg) = allow_block_list.is_ip_allowed(&dbg!(remote_addr)) {
                let _ = match self.runtime.block_on(server.send_error(error_msg.clone())) {
                    Err(_) => Err(std::io::Error::new(
                        std::io::ErrorKind::PermissionDenied,
                        format!("{}  timed out", &error_msg),
                    )
                    .into()),
                    Ok(Err(e)) => Err(e),
                    Ok(Ok(_)) => Ok(()),
                };
                // not exactly sure what to do here
                // return Err(std::io::Error::new(
                //     std::io::ErrorKind::PermissionDenied,
                //     error_msg,
                // ));
                continue;
            };

            // TODO: find a better way to track count
            // the `- 1` is to account for the top-level Arc that is created at the top
            // of this method. subsequent counts correspond to each `clone` that is passed
            // into a thread
            // TODO: If we don't find a way to handle the counting automagically, make
            //       a dedicated wrapper-type with doc-comments, manual drop impl that
            //       integrates logging, etc...
            if Arc::strong_count(&bootstrap_sessions_counter) - 1 < max_bootstraps {
                massa_trace!("bootstrap.lib.run.select.accept", {
                    "remote_addr": remote_addr
                });
                let now = Instant::now();

                // clear IP history if necessary
                if self.ip_hist_map.len() > self.bootstrap_config.ip_list_max_size {
                    self.ip_hist_map
                        .retain(|_k, v| now.duration_since(*v) <= per_ip_min_interval);
                    if self.ip_hist_map.len() > self.bootstrap_config.ip_list_max_size {
                        // too many IPs are spamming us: clear cache
                        warn!("high bootstrap load: at least {} different IPs attempted bootstrap in the last {}", self.ip_hist_map.len(),format_duration(self.bootstrap_config.per_ip_min_interval.to_duration()).to_string());
                        self.ip_hist_map.clear();
                    }
                }

                // check IP's bootstrap attempt history
                if let Err(msg) = BootstrapServer::greedy_client_check(
                    &mut self.ip_hist_map,
                    remote_addr,
                    now,
                    per_ip_min_interval,
                ) {
                    let send_timeout = server.send_error(

                            format!(
                                "Your last bootstrap on this server was {} ago and you have to wait {} before retrying.",
                                format_duration(msg),
                                format_duration(per_ip_min_interval.saturating_sub(msg))
                            )

                    );
                    let send_timeout = self.runtime.block_on(send_timeout);

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

                self.runtime.spawn(run_bootstrap_session(
                    server,
                    bootstrap_count_token,
                    config,
                    remote_addr,
                    data_execution,
                    version,
                    consensus_command_sender,
                    network_command_sender,
                ));

                massa_trace!("bootstrap.session.started", {
                    "active_count": Arc::strong_count(&bootstrap_sessions_counter) - 1
                });
            } else {
                let _ = match  self.runtime.block_on(server.send_error("Bootstrap failed because the bootstrap server currently has no slots available.".to_string())) {
                            Err(_) => Err(std::io::Error::new(std::io::ErrorKind::TimedOut, "bootstrap error no available slots send timed out").into()),
                            Ok(Err(e)) => Err(e),
                            Ok(Ok(_)) => Ok(()),
                        };
                debug!("did not bootstrap {}: no available slots", remote_addr);
            }
        }
    }

    /// Helper method to check if this IP is being greedy, i.e. not enough elapsed time since last attempt
    ///
    /// # Error
    /// The elapsed time which is insufficient
    fn greedy_client_check(
        ip_hist_map: &mut HashMap<IpAddr, Instant>,
        remote_addr: SocketAddr,
        now: Instant,
        per_ip_min_interval: Duration,
    ) -> Result<(), Duration> {
        let mut res = Ok(());
        ip_hist_map
            .entry(remote_addr.ip())
            .and_modify(|occ| {
                if now.duration_since(*occ) <= per_ip_min_interval {
                    res = Err(occ.elapsed());
                } else {
                    // in list, expired
                    *occ = now;
                }
            })
            .or_insert(now);
        res
    }
}

/// To be called from a `tokio::spawn` invocation
///
/// Runs the bootstrap management in a tokio::timeout context. A failed bootstrap session
/// will fail silently locally, but will send a message to the client with an error message.
///
/// The arc_counter variable is used as a proxy to keep track the number of active bootstrap
/// sessions.
#[allow(clippy::too_many_arguments)]
async fn run_bootstrap_session(
    mut server: BootstrapServerBinder,
    arc_counter: Arc<()>,
    config: BootstrapConfig,
    remote_addr: SocketAddr,
    data_execution: Arc<RwLock<FinalState>>,
    version: Version,
    consensus_command_sender: Box<dyn ConsensusController>,
    network_command_sender: NetworkCommandSender,
) {
    panic!("running session");
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
    // This drop allows the server to accept new connections before having to complete the error notifications
    massa_trace!("bootstrap.session.finished", {
        "active_count": Arc::strong_count(&arc_counter) - 1
    });
    drop(arc_counter);
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
                let _ = server.send_error(err.to_string()).await;
            }
        },
        Err(_timeout) => {
            debug!("bootstrap timeout for peer {}", remote_addr);
            // We allow unused result because we don't care if an error is thrown when
            // sending the error message to the server we will close the socket anyway.
            let _ = server
                .send_error(format!(
                    "Bootstrap process timedout ({})",
                    format_duration(config.bootstrap_timeout.to_duration())
                ))
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

// Stable means of providing compiler optimisation hints
// Also provides a self-documenting tool to communicate likely/unlikely code-paths
// https://users.rust-lang.org/t/compiler-hint-for-unlikely-likely-for-if-branches/62102/4
#[inline]
#[cold]
fn cold() {}

#[inline]
fn _likely(b: bool) -> bool {
    if !b {
        cold()
    }
    b
}

#[inline]
fn unlikely(b: bool) -> bool {
    if b {
        cold()
    }
    b
}
