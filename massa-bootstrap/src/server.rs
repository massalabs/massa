//! start the bootstrapping system using [`start_bootstrap_server`]
//! Once your node will be ready, you may want other to bootstrap from you.
//!
//! # Listener
//!
//! Runs in the server-dedication tokio async runtime
//! Accepts bootstrap connections in an async-loop
//! Upon connection, pushes the accepted connection onto a channel for the worker loop to consume
//!
//! # Updater
//!
//! Runs on a dedicated thread. Signal sent my manager stop method terminates the thread.
//! Shares an `Arc<RwLock>>` guarded list of white and blacklists with the main worker.
//! Periodically does a read-only check to see if list needs updating.
//! Creates an updated list then swaps it out with write-locked list
//! Assuming no errors in code, this is the only write occurance, and is only a pointer-swap
//! under the hood, making write contention virtually non-existant.
//!
//! # Worker loop
//!
//! 1. Checks if the stopper has been invoked.
//! 2. Checks if the client is permited under the white/black list rules
//! 3. Checks if there are not too many active sessions already
//! 4. Checks if the client has attempted too recently
//! 5. All checks have passed: spawn a thread on which to run the bootstrap session
//!    This thread creates a new tokio runtime, and runs it with `block_on`
mod white_black_list;

use crossbeam::channel::{tick, Select, SendError};
use humantime::format_duration;
use massa_async_pool::AsyncMessageId;
use massa_consensus_exports::{bootstrapable_graph::BootstrapableGraph, ConsensusController};
use massa_final_state::{FinalState, FinalStateError};
use massa_ledger_exports::Key as LedgerKey;
use massa_logging::massa_trace;
use massa_models::{
    block_id::BlockId, prehash::PreHashSet, slot::Slot, streaming_step::StreamingStep,
    version::Version,
};

#[cfg(any(test, feature = "test"))]
use massa_network_exports::MockNetworkCommandSender as NetworkCommandSender;
#[cfg(not(any(test, feature = "test")))]
use massa_network_exports::NetworkCommandSender;
use massa_signature::KeyPair;
use massa_time::MassaTime;
use massa_versioning_worker::versioning::MipStore;

use parking_lot::RwLock;
use std::{
    collections::HashMap,
    net::{IpAddr, SocketAddr, TcpStream},
    sync::Arc,
    thread,
    time::{Duration, Instant},
};
use tracing::{debug, error, info, warn};
use white_black_list::*;

use crate::{
    error::BootstrapError,
    establisher::BSListener,
    messages::{BootstrapClientMessage, BootstrapServerMessage},
    server_binder::BootstrapServerBinder,
    BootstrapConfig,
};

/// Abstraction layer over data produced by the listener, and transported
/// over to the worker via a channel
type BsConn = (TcpStream, SocketAddr);

/// handle on the bootstrap server
pub struct BootstrapManager {
    update_handle: thread::JoinHandle<Result<(), BootstrapError>>,
    // need to preserve the listener handle up to here to prevent it being destroyed
    #[allow(clippy::type_complexity)]
    _listen_handle: thread::JoinHandle<Result<Result<(), BsConn>, BootstrapError>>,
    main_handle: thread::JoinHandle<Result<(), BootstrapError>>,
    listen_stopper_tx: crossbeam::channel::Sender<()>,
    update_stopper_tx: crossbeam::channel::Sender<()>,
}

impl BootstrapManager {
    /// stop the bootstrap server
    pub fn stop(self) -> Result<(), BootstrapError> {
        massa_trace!("bootstrap.lib.stop", {});
        if self.listen_stopper_tx.send(()).is_err() {
            warn!("bootstrap server already dropped");
        }
        if self.update_stopper_tx.send(()).is_err() {
            warn!("bootstrap ip-list-updater already dropped");
        }
        // TODO?: handle join errors.

        // when the runtime is dropped at the end of this stop, the listener is auto-aborted

        self.update_handle
            .join()
            .expect("in BootstrapManager::stop() joining on updater thread")?;

        self.main_handle
            .join()
            .expect("in BootstrapManager::stop() joining on bootstrap main-loop thread")
    }
}

/// See module level documentation for details
#[allow(clippy::too_many_arguments)]
pub fn start_bootstrap_server(
    consensus_controller: Box<dyn ConsensusController>,
    network_command_sender: NetworkCommandSender,
    final_state: Arc<RwLock<FinalState>>,
    config: BootstrapConfig,
    listener: impl BSListener + Send + 'static,
    keypair: KeyPair,
    version: Version,
    mip_store: MipStore,
) -> Result<Option<BootstrapManager>, BootstrapError> {
    massa_trace!("bootstrap.lib.start_bootstrap_server", {});

    // TODO(low prio): See if a zero capacity channel model can work
    let (listen_stopper_tx, listen_stopper_rx) = crossbeam::channel::bounded::<()>(1);
    let (update_stopper_tx, update_stopper_rx) = crossbeam::channel::bounded::<()>(1);

    let Ok(max_bootstraps) = config.max_simultaneous_bootstraps.try_into() else {
        return Err(BootstrapError::GeneralError("Fail to convert u32 to usize".to_string()));
    };

    // This is needed for now, as there is no ergonomic to "select!" on both a channel and a blocking std::net::TcpStream
    let (listener_tx, listener_rx) = crossbeam::channel::bounded::<BsConn>(max_bootstraps * 2);

    let white_black_list = SharedWhiteBlackList::new(
        config.bootstrap_whitelist_path.clone(),
        config.bootstrap_blacklist_path.clone(),
    )?;

    let updater_lists = white_black_list.clone();
    let update_handle = thread::Builder::new()
        .name("wb_list_updater".to_string())
        .spawn(move || {
            let res = BootstrapServer::run_updater(
                updater_lists,
                config.cache_duration.into(),
                update_stopper_rx,
            );
            match res {
                Ok(_) => info!("ip white/blacklist updater exited cleanly"),
                Err(ref er) => error!("updater exited with error: {}", er),
            };
            res
        })
        .expect("in `start_bootstrap_server`, OS failed to spawn list-updater thread");
    let listen_handle = thread::Builder::new()
        .name("bs_listener".to_string())
        // FIXME: The interface being used shouldn't have `: Send + 'static` as a constraint on the listener assosciated type.
        // GAT lifetime is likely to remedy this, however.
        .spawn(move || BootstrapServer::run_listener(listener, listener_tx))
        .expect("in `start_bootstrap_server`, OS failed to spawn listener thread");

    let main_handle = thread::Builder::new()
        .name("bs-main-loop".to_string())
        .spawn(move || {
            BootstrapServer {
                consensus_controller,
                network_command_sender,
                final_state,
                listener_rx,
                listen_stopper_rx,
                white_black_list,
                keypair,
                version,
                ip_hist_map: HashMap::with_capacity(config.ip_list_max_size),
                bootstrap_config: config,
                mip_store,
            }
            .run_loop(max_bootstraps)
        })
        .expect("in `start_bootstrap_server`, OS failed to spawn main-loop thread");
    // Give the runtime to the bootstrap manager, otherwise it will be dropped, forcibly aborting the spawned tasks.
    // TODO: make the tasks sync, so the runtime is redundant
    Ok(Some(BootstrapManager {
        update_handle,
        _listen_handle: listen_handle,
        main_handle,
        listen_stopper_tx,
        update_stopper_tx,
    }))
}

struct BootstrapServer<'a> {
    consensus_controller: Box<dyn ConsensusController>,
    network_command_sender: NetworkCommandSender,
    final_state: Arc<RwLock<FinalState>>,
    listener_rx: crossbeam::channel::Receiver<BsConn>,
    listen_stopper_rx: crossbeam::channel::Receiver<()>,
    white_black_list: SharedWhiteBlackList<'a>,
    keypair: KeyPair,
    bootstrap_config: BootstrapConfig,
    version: Version,
    ip_hist_map: HashMap<IpAddr, Instant>,
    mip_store: MipStore,
}

impl BootstrapServer<'_> {
    fn run_updater(
        mut list: SharedWhiteBlackList<'_>,
        interval: Duration,
        stopper: crossbeam::channel::Receiver<()>,
    ) -> Result<(), BootstrapError> {
        let ticker = tick(interval);

        loop {
            crossbeam::select! {
                recv(stopper) -> res => {
                    match res {
                        Ok(()) => return Ok(()),
                        Err(e) => return Err(BootstrapError::GeneralError(format!("update stopper error : {}", e))),
                    }
                },
                recv(ticker) -> _ => {list.update()?;},
            }
        }
    }

    /// Listens on a channel for incoming connections, and loads them onto a crossbeam channel
    /// for the main-loop to process.
    ///
    /// Ok(Ok(())) listener closed without issue
    /// Ok(Err((dplx, address))) listener accepted a connection then tried sending on a disconnected channel
    /// Err(..) Error accepting a connection
    /// TODO: Integrate the listener into the bootstrap-main-loop
    fn run_listener(
        mut listener: impl BSListener,
        listener_tx: crossbeam::channel::Sender<BsConn>,
    ) -> Result<Result<(), BsConn>, BootstrapError> {
        loop {
            let (msg, addr) = listener.accept().map_err(BootstrapError::IoError)?;

            if let Err(SendError((dplx, remote_addr))) = listener_tx.send((msg, addr)) {
                warn!(
                    "listener channel disconnected after accepting connection from {}",
                    remote_addr
                );
                return Ok(Err((dplx, remote_addr)));
            };
        }
    }

    fn run_loop(mut self, max_bootstraps: usize) -> Result<(), BootstrapError> {
        // Use the strong-count of this variable to track the session count
        let bootstrap_sessions_counter: Arc<()> = Arc::new(());
        let per_ip_min_interval = self.bootstrap_config.per_ip_min_interval.to_duration();
        let mut selector = Select::new();
        selector.recv(&self.listen_stopper_rx);
        selector.recv(&self.listener_rx);
        // TODO: Work out how to integration-test this
        loop {
            // block until we have a connection to work with, or break out of main-loop
            // if a stop-signal is received
            let Some((dplx, remote_addr)) = self.receive_connection(&mut selector)? else { break; };
            // claim a slot in the max_bootstrap_sessions
            let server_binding = BootstrapServerBinder::new(
                dplx,
                self.keypair.clone(),
                (&self.bootstrap_config).into(),
            );

            // check whether incoming peer IP is allowed.
            if let Err(error_msg) = self.white_black_list.is_ip_allowed(&remote_addr) {
                server_binding.close_and_send_error(error_msg.to_string(), remote_addr, move || {});
                continue;
            };

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
                    // Client has been too greedy: send out the bad-news :(
                    let msg = format!(
                        "Your last bootstrap on this server was {} ago and you have to wait {} before retrying.",
                        format_duration(msg),
                        format_duration(per_ip_min_interval.saturating_sub(msg))
                    );
                    let tracer = move || {
                        massa_trace!("bootstrap.lib.run.select.accept.refuse_limit", {
                            "remote_addr": remote_addr
                        })
                    };
                    server_binding.close_and_send_error(msg, remote_addr, tracer);
                    continue;
                };

                // Clients Option<last-attempt> is good, and has been updated
                massa_trace!("bootstrap.lib.run.select.accept.cache_available", {});

                // launch bootstrap
                let version = self.version;
                let data_execution = self.final_state.clone();
                let consensus_command_sender = self.consensus_controller.clone();
                let network_command_sender = self.network_command_sender.clone();
                let config = self.bootstrap_config.clone();

                let bootstrap_count_token = bootstrap_sessions_counter.clone();
                let mip_store = self.mip_store.clone();

                let _ = thread::Builder::new()
                    .name(format!("bootstrap thread, peer: {}", remote_addr))
                    .spawn(move || {
                        run_bootstrap_session(
                            server_binding,
                            bootstrap_count_token,
                            config,
                            remote_addr,
                            data_execution,
                            version,
                            consensus_command_sender,
                            network_command_sender,
                            mip_store,
                        )
                    });

                massa_trace!("bootstrap.session.started", {
                    "active_count": Arc::strong_count(&bootstrap_sessions_counter) - 1
                });
            } else {
                server_binding.close_and_send_error(
                    "Bootstrap failed because the bootstrap server currently has no slots available.".to_string(),
                    remote_addr,
                    move || debug!("did not bootstrap {}: no available slots", remote_addr),
                );
            }
        }

        // Give any remaining processes 20 seconds to clean up, otherwise force them to shutdown
        Ok(())
    }

    /// These are the steps to ensure that a connection is only processed if the server is active:
    ///
    /// - 1. Block until _something_ is ready
    /// - 2. If that something is a stop-signal, stop
    /// - 3. If that something is anything else:
    /// - 3.a. double check the stop-signal is absent
    /// - 3.b. If present, fall-back to the stop behaviour
    /// - 3.c. If absent, all's clear to rock-n-roll.
    fn receive_connection(&self, selector: &mut Select) -> Result<Option<BsConn>, BootstrapError> {
        // 1. Block until _something_ is ready
        let rdy = selector.ready();

        // 2. If that something is a stop-signal, stop
        // from `crossbeam::Select::read()` documentation:
        // "Note that this method might return with success spuriously, so it’s a good idea
        // to always double check if the operation is really ready."
        if rdy == 0 && self.listen_stopper_rx.try_recv().is_ok() {
            return Ok(None);
            // - 3. If that something is anything else:
        } else {
            massa_trace!("bootstrap.lib.run.select", {});

            // - 3.a. double check the stop-signal is absent
            let stop = self.listen_stopper_rx.try_recv();
            if unlikely(stop.is_ok()) {
                massa_trace!("bootstrap.lib.run.select.manager", {});
                // 3.b. If present, fall-back to the stop behaviour
                return Ok(None);
            } else if unlikely(stop == Err(crossbeam::channel::TryRecvError::Disconnected)) {
                return Err(BootstrapError::GeneralError(
                    "Unexpected stop-channel disconnection".to_string(),
                ));
            };
        }
        // - 3.c. If absent, all's clear to rock-n-roll.
        let msg = match self.listener_rx.try_recv() {
            Ok(msg) => msg,
            Err(try_rcv_err) => match try_rcv_err {
                crossbeam::channel::TryRecvError::Empty => return Ok(None),
                crossbeam::channel::TryRecvError::Disconnected => {
                    return Err(BootstrapError::GeneralError(
                        "listener recv channel disconnected unexpectedly".to_string(),
                    ));
                }
            },
        };
        Ok(Some(msg))
    }

    /// Checks latest attempt. If too recent, provides the bad news (as an error).
    /// Updates the latest attempt to "now" if it's all good.
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
                // Well, let's only update the latest
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

/// To be called from a `thread::spawn` invocation
///
/// Runs the bootstrap management in a dedicated thread, handling the async by using
/// a multi-thread-aware tokio runtime (the bs-main-loop runtime, to be exact). When this
/// function blocks in the `block_on`, it should thread-block, and switch to another session
///
/// The arc_counter variable is used as a proxy to keep track the number of active bootstrap
/// sessions.
#[allow(clippy::too_many_arguments)]
fn run_bootstrap_session(
    mut server: BootstrapServerBinder,
    arc_counter: Arc<()>,
    config: BootstrapConfig,
    remote_addr: SocketAddr,
    data_execution: Arc<RwLock<FinalState>>,
    version: Version,
    consensus_command_sender: Box<dyn ConsensusController>,
    network_command_sender: NetworkCommandSender,
    mip_store: MipStore,
) {
    debug!("running bootstrap for peer {}", remote_addr);
    let deadline = Instant::now() + config.bootstrap_timeout.to_duration();
    // TODO: reinstate prevention of bootstrap slot camping. Deadline cancellation is one option
    let res = manage_bootstrap(
        &config,
        &mut server,
        data_execution,
        version,
        consensus_command_sender,
        network_command_sender,
        deadline,
        mip_store,
    );

    // This drop allows the server to accept new connections before having to complete the error notifications
    // account for this session being finished, as well as the root-instance
    massa_trace!("bootstrap.session.finished", {
        "sessions_remaining": Arc::strong_count(&arc_counter) - 2
    });
    drop(arc_counter);
    match res {
        Err(BootstrapError::TimedOut(_)) => {
            debug!("bootstrap timeout for peer {}", remote_addr);
            // We allow unused result because we don't care if an error is thrown when
            // sending the error message to the server we will close the socket anyway.
            let _ = server.send_error_timeout(format!(
                "Bootstrap process timedout ({})",
                format_duration(config.bootstrap_timeout.to_duration())
            ));
        }
        Err(BootstrapError::ReceivedError(error)) => debug!(
            "bootstrap serving error received from peer {}: {}",
            remote_addr, error
        ),
        Err(err) => {
            debug!("bootstrap serving error for peer {}: {}", remote_addr, err);
            // We allow unused result because we don't care if an error is thrown when
            // sending the error message to the server we will close the socket anyway.
            let _ = server.send_error_timeout(err.to_string());
        }
        Ok(_) => {
            info!("bootstrapped peer {}", remote_addr);
        }
    }
}

#[allow(clippy::too_many_arguments)]
pub fn stream_bootstrap_information(
    server: &mut BootstrapServerBinder,
    final_state: Arc<RwLock<FinalState>>,
    consensus_controller: Box<dyn ConsensusController>,
    mut last_slot: Option<Slot>,
    mut last_ledger_step: StreamingStep<LedgerKey>,
    mut last_pool_step: StreamingStep<AsyncMessageId>,
    mut last_cycle_step: StreamingStep<u64>,
    mut last_credits_step: StreamingStep<Slot>,
    mut last_ops_step: StreamingStep<Slot>,
    mut last_de_step: StreamingStep<Slot>,
    mut last_consensus_step: StreamingStep<PreHashSet<BlockId>>,
    mut send_last_start_period: bool,
    bs_deadline: &Instant,
    write_timeout: Duration,
) -> Result<(), BootstrapError> {
    loop {
        #[cfg(test)]
        {
            // Necessary for test_bootstrap_server in tests/scenarios.rs
            std::thread::sleep(Duration::from_millis(500));
        }

        let current_slot;
        let ledger_part;
        let async_pool_part;
        let pos_cycle_part;
        let pos_credits_part;
        let exec_ops_part;
        let processed_de_part;
        let final_state_changes;
        let last_start_period;

        let mut slot_too_old = false;

        // Scope of the final state read
        {
            let final_state_read = final_state.read();

            last_start_period = if send_last_start_period {
                Some(final_state_read.last_start_period)
            } else {
                None
            };

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

            let (de_data, new_de_step) = final_state_read
                .executed_denunciations
                .get_processed_de_part(last_de_step);
            processed_de_part = de_data;

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
                    new_de_step,
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
            last_de_step = new_de_step;
            last_slot = Some(final_state_read.slot);
            current_slot = final_state_read.slot;
            send_last_start_period = false;
        }

        if slot_too_old {
            return server.send_msg(write_timeout, BootstrapServerMessage::SlotTooOld);
        }

        // Setup final state global cursor
        let final_state_global_step = if last_ledger_step.finished()
            && last_pool_step.finished()
            && last_cycle_step.finished()
            && last_credits_step.finished()
            && last_ops_step.finished()
            && last_de_step.finished()
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
        // We don't bother with the bs-deadline, as this is the last step of the bootstrap process - defer to general write-timeout
        if final_state_global_step.finished()
            && final_state_changes_step.finished()
            && last_consensus_step.finished()
        {
            server.send_msg(write_timeout, BootstrapServerMessage::BootstrapFinished)?;
            break;
        }

        let Some(write_timeout) = step_timeout_duration(bs_deadline, &write_timeout) else {
            return Err(BootstrapError::Interupted("insufficient time left to provide next bootstrap part".to_string()));
        };
        // At this point we know that consensus, final state or both are not finished
        server.send_msg(
            write_timeout,
            BootstrapServerMessage::BootstrapPart {
                slot: current_slot,
                ledger_part,
                async_pool_part,
                pos_cycle_part,
                pos_credits_part,
                exec_ops_part,
                processed_de_part,
                final_state_changes,
                consensus_part,
                consensus_outdated_ids,
                last_start_period,
            },
        )?;
    }
    Ok(())
}

// derives the duration allowed for a step in the bootstrap process.
// Returns None if the deadline for the entire bs-process has been reached
fn step_timeout_duration(bs_deadline: &Instant, step_timeout: &Duration) -> Option<Duration> {
    let now = Instant::now();
    if now >= *bs_deadline {
        return None;
    }

    let remaining = *bs_deadline - now;
    Some(std::cmp::min(remaining, *step_timeout))
}
#[allow(clippy::too_many_arguments)]
fn manage_bootstrap(
    bootstrap_config: &BootstrapConfig,
    server: &mut BootstrapServerBinder,
    final_state: Arc<RwLock<FinalState>>,
    version: Version,
    consensus_controller: Box<dyn ConsensusController>,
    network_command_sender: NetworkCommandSender,
    deadline: Instant,
    mip_store: MipStore,
) -> Result<(), BootstrapError> {
    massa_trace!("bootstrap.lib.manage_bootstrap", {});
    let read_error_timeout: Duration = bootstrap_config.read_error_timeout.into();

    let Some(hs_timeout) = step_timeout_duration(&deadline, &bootstrap_config.read_timeout.to_duration()) else {
        return Err(BootstrapError::Interupted("insufficient time left to begin handshake".to_string()));
    };

    server.handshake_timeout(version, Some(hs_timeout))?;

    // Check for error from client
    if Instant::now() + read_error_timeout >= deadline {
        return Err(BootstrapError::Interupted(
            "insufficient time to check for error from client".to_string(),
        ));
    };
    match server.next_timeout(Some(read_error_timeout)) {
        Err(BootstrapError::TimedOut(_)) => {}
        Err(e) => return Err(e),
        Ok(BootstrapClientMessage::BootstrapError { error }) => {
            return Err(BootstrapError::GeneralError(error));
        }
        Ok(msg) => return Err(BootstrapError::UnexpectedClientMessage(Box::new(msg))),
    };

    // Sync clocks
    let send_time_timeout =
        step_timeout_duration(&deadline, &bootstrap_config.write_timeout.to_duration());
    let Some(next_step_timeout) = send_time_timeout else {
        return Err(BootstrapError::Interupted("insufficient time left to send server time".to_string()));
    };
    server.send_msg(
        next_step_timeout,
        BootstrapServerMessage::BootstrapTime {
            server_time: MassaTime::now()?,
            version,
        },
    )?;

    loop {
        let Some(read_timeout) = step_timeout_duration(&deadline, &bootstrap_config.read_timeout.to_duration()) else {
            return Err(BootstrapError::Interupted("insufficient time left to process next message".to_string()));
        };
        match server.next_timeout(Some(read_timeout)) {
            Err(BootstrapError::TimedOut(_)) => break Ok(()),
            Err(e) => break Err(e),
            Ok(msg) => match msg {
                BootstrapClientMessage::AskBootstrapPeers => {
                    let Some(write_timeout) = step_timeout_duration(&deadline, &bootstrap_config.write_timeout.to_duration()) else {
                        return Err(BootstrapError::Interupted("insufficient time left to respond te request for peers".to_string()));
                    };

                    server.send_msg(
                        write_timeout,
                        BootstrapServerMessage::BootstrapPeers {
                            peers: network_command_sender.sync_get_bootstrap_peers()?,
                        },
                    )?;
                }
                BootstrapClientMessage::AskBootstrapPart {
                    last_slot,
                    last_ledger_step,
                    last_pool_step,
                    last_cycle_step,
                    last_credits_step,
                    last_ops_step,
                    last_de_step,
                    last_consensus_step,
                    send_last_start_period,
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
                        last_de_step,
                        last_consensus_step,
                        send_last_start_period,
                        &deadline,
                        bootstrap_config.write_timeout.to_duration(),
                    )?;
                }
                BootstrapClientMessage::AskBootstrapMipStore => {
                    let vs = mip_store.0.read().to_owned();
                    let Some(write_timeout) = step_timeout_duration(&deadline, &bootstrap_config.write_timeout.to_duration()) else {
                        return Err(BootstrapError::Interupted("insufficient time left to respond te request for mip-store".to_string()));
                    };

                    server.send_msg(
                        write_timeout,
                        BootstrapServerMessage::BootstrapMipStore { store: vs.clone() },
                    )?
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
