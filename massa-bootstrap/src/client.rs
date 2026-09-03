use humantime::format_duration;
use massa_db_exports::DBBatch;
use massa_final_state::{FinalStateController, FinalStateError};
use massa_logging::massa_trace;
use massa_metrics::MassaMetrics;
use massa_models::{
    node::NodeId, slot::Slot, streaming_step::StreamingStep, timeslots::get_block_slot_timestamp,
    version::Version,
};
use massa_signature::PublicKey;
use massa_time::MassaTime;
use massa_versioning::versioning::{ComponentStateTypeId, MipInfo, MipState, StateAtError};
use parking_lot::RwLock;
use rand::{
    prelude::{SliceRandom, StdRng},
    SeedableRng,
};
use std::collections::BTreeMap;
use std::{
    collections::HashSet,
    io,
    net::{SocketAddr, TcpStream},
    sync::{Arc, Condvar, Mutex},
    time::Duration,
};
use tracing::{debug, info, warn};

use crate::{
    bindings::BootstrapClientBinder,
    error::BootstrapError,
    messages::{BootstrapClientMessage, BootstrapServerMessage},
    settings::IpType,
    BootstrapConfig, GlobalBootstrapState,
};

/// Specifies a common interface that can be used by standard, or mockers
#[cfg_attr(test, mockall::automock)]
pub trait BSConnector {
    /// The client attempts to connect to the given address.
    /// If a duration is provided, the attempt will be timed out after the given duration.
    fn connect_timeout(
        &self,
        addr: SocketAddr,
        duration: Option<MassaTime>,
    ) -> io::Result<TcpStream>;
}

/// Initiates a connection with given timeout in milliseconds
#[derive(Debug)]
pub struct DefaultConnector;

impl BSConnector for DefaultConnector {
    /// Tries to connect to address
    ///
    /// # Argument
    /// * `addr`: `SocketAddr` we are trying to connect to.
    fn connect_timeout(
        &self,
        addr: SocketAddr,
        duration: Option<MassaTime>,
    ) -> io::Result<TcpStream> {
        let Some(duration) = duration else {
            return TcpStream::connect(addr);
        };
        TcpStream::connect_timeout(&addr, duration.to_duration())
    }
}
/// This function will send the starting point to receive a stream of the ledger and will receive and process each part until receive a `BootstrapServerMessage::FinalStateFinished` message from the server.
/// `next_bootstrap_message` passed as parameter must be `BootstrapClientMessage::AskFinalStatePart` enum variant.
/// `next_bootstrap_message` will be updated after receiving each part so that in case of connection lost we can restart from the last message we processed.
pub(crate) fn stream_final_state_and_consensus(
    cfg: &BootstrapConfig,
    client: &mut BootstrapClientBinder,
    next_bootstrap_message: &mut BootstrapClientMessage,
    global_bootstrap_state: &mut GlobalBootstrapState,
) -> Result<(), BootstrapError> {
    if let BootstrapClientMessage::AskBootstrapPart {
        send_last_start_period: false,
        ..
    } = &next_bootstrap_message
    {
        // Continuation / reconnect: metadata was received on the first part (empty
        // consensus); seed the binder for block header validation while parsing.
        client.set_last_start_period(Some(
            global_bootstrap_state
                .final_state
                .read()
                .get_last_start_period(),
        ));
    } else if let BootstrapClientMessage::AskBootstrapPart {
        send_last_start_period: true,
        ..
    } = &next_bootstrap_message
    {
        client.set_last_start_period(None);
    }

    if let BootstrapClientMessage::AskBootstrapPart { .. } = &next_bootstrap_message {
        client.send_timeout(
            next_bootstrap_message,
            Some(cfg.write_timeout.to_duration()),
        )?;

        loop {
            match client.next_timeout(Some(cfg.read_timeout.to_duration()))? {
                BootstrapServerMessage::BootstrapPart {
                    slot,
                    state_part,
                    versioning_part,
                    consensus_part,
                    consensus_outdated_ids,
                    last_start_period,
                    last_slot_before_downtime,
                } => {
                    // Set final state
                    let mut write_final_state = global_bootstrap_state.final_state.write();

                    // Reject inconsistent network restart metadata before storing it: both
                    // fields feed slot and timestamp arithmetic at startup, so impossible values
                    // would only surface much later, as a panic following an otherwise
                    // successful bootstrap.
                    check_restart_metadata(cfg, &last_start_period, &last_slot_before_downtime)?;

                    if let Some(last_start_period) = last_start_period {
                        write_final_state.set_last_start_period(last_start_period);
                        client.set_last_start_period(Some(last_start_period));
                    }
                    if let Some(last_slot_before_downtime) = last_slot_before_downtime {
                        write_final_state.set_last_slot_before_downtime(last_slot_before_downtime);
                    }

                    let (last_state_step, last_versioning_step) = write_final_state
                        .get_database()
                        .write()
                        .write_batch_bootstrap_client(state_part, versioning_part)
                        .map_err(|e| {
                            BootstrapError::GeneralError(format!(
                                "Cannot write received stream batch to disk: {}",
                                e
                            ))
                        })?;

                    // Set consensus blocks
                    if let Some(graph) = global_bootstrap_state.graph.as_mut() {
                        // Extend the final blocks with the received part
                        graph.final_blocks.extend(consensus_part.final_blocks);
                        // Remove every outdated block
                        graph.final_blocks.retain(|block_export| {
                            !consensus_outdated_ids.contains(&block_export.block.id)
                        });
                    } else {
                        global_bootstrap_state.graph = Some(consensus_part);
                    }
                    let last_consensus_step = StreamingStep::Ongoing(
                        // Note that this unwrap call is safe because of the above conditional statement
                        global_bootstrap_state
                            .graph
                            .as_ref()
                            .unwrap()
                            .final_blocks
                            .iter()
                            .map(|b_export| b_export.block.id)
                            .collect(),
                    );

                    // Set new message in case of disconnection
                    *next_bootstrap_message = BootstrapClientMessage::AskBootstrapPart {
                        last_slot: Some(slot),
                        last_state_step,
                        last_versioning_step,
                        last_consensus_step,
                        send_last_start_period: false,
                    };

                    // Logs for an easier diagnostic if needed
                    debug!(
                        "client final state bootstrap cursors: {:?}",
                        next_bootstrap_message
                    );
                }
                BootstrapServerMessage::BootstrapFinished => {
                    info!("State bootstrap complete");

                    // Update MIP store by reading from the disk
                    let mut guard = global_bootstrap_state.final_state.write();
                    let db = guard.get_database().clone();
                    let (updated, added) = guard
                        .get_mip_store_mut()
                        .extend_from_db(db)
                        .map_err(|e| BootstrapError::from(FinalStateError::from(e)))?;

                    warn_user_about_versioning_updates(updated, added);

                    // The downtime range announced by the server must also be consistent with the
                    // MIP store we just bootstrapped: startup requires it, so checking it here
                    // lets us retry with another server instead of failing after the fact.
                    if let Some(last_slot_before_downtime) = *guard.get_last_slot_before_downtime()
                    {
                        let (shutdown_start, shutdown_end) = shutdown_range(
                            cfg,
                            guard.get_last_start_period(),
                            last_slot_before_downtime,
                        )?;
                        guard
                            .get_mip_store()
                            .is_consistent_with_shutdown_period(
                                shutdown_start,
                                shutdown_end,
                                cfg.thread_count,
                                cfg.t0,
                                cfg.genesis_timestamp,
                            )
                            .map_err(|e| {
                                BootstrapError::GeneralError(format!(
                                    "bootstrapped MIP store is not consistent with the shutdown period announced by the server: {}",
                                    e
                                ))
                            })?;
                    }

                    // Only advance to the next phase once the streamed state has been fully
                    // post-processed: on failure the retry must replay the state streaming
                    // instead of resuming from the peers phase with a half-updated store.
                    *next_bootstrap_message = BootstrapClientMessage::AskBootstrapPeers;

                    return Ok(());
                }
                BootstrapServerMessage::SlotTooOld => {
                    info!("Slot is too old retry bootstrap from scratch");
                    *next_bootstrap_message = BootstrapClientMessage::AskBootstrapPart {
                        last_slot: None,
                        last_state_step: StreamingStep::Started,
                        last_versioning_step: StreamingStep::Started,
                        last_consensus_step: StreamingStep::Started,
                        send_last_start_period: true,
                    };
                    let mut write_final_state = global_bootstrap_state.final_state.write();
                    write_final_state.reset();
                    // `reset()` does not clear restart metadata; drop values from the
                    // aborted server so the next one defines them from the wire again.
                    write_final_state.set_last_start_period(0);
                    write_final_state.set_last_slot_before_downtime(None);
                    drop(write_final_state);
                    client.set_last_start_period(None);
                    // The cursor above restarts the consensus stream from `Started`, for which the
                    // server reports no outdated ids: blocks kept from the aborted attempt would
                    // never be pruned and would be merged into the next attempt's graph.
                    global_bootstrap_state.graph = None;
                    global_bootstrap_state.peers = None;
                    return Err(BootstrapError::GeneralError(String::from("Slot too old")));
                }
                // At this point, we have successfully received the next message from the server, and it's an error-message String
                BootstrapServerMessage::BootstrapError { error } => {
                    return Err(BootstrapError::GeneralError(error))
                }
                _ => {
                    return Err(BootstrapError::GeneralError(
                        "unexpected message".to_string(),
                    ))
                }
            }
        }
    } else {
        Err(BootstrapError::GeneralError(format!(
            "Try to stream the final state but the message to send to the server was {:#?}",
            next_bootstrap_message
        )))
    }
}

/// Gets the state from a bootstrap server (internal private function)
/// needs to be CANCELLABLE
pub(crate) fn bootstrap_from_server(
    cfg: &BootstrapConfig,
    client: &mut BootstrapClientBinder,
    next_bootstrap_message: &mut BootstrapClientMessage,
    global_bootstrap_state: &mut GlobalBootstrapState,
    our_version: Version,
) -> Result<(), BootstrapError> {
    massa_trace!("bootstrap.lib.bootstrap_from_server", {});

    // read error (if sent by the server)
    // client.next() is not cancel-safe but we drop the whole client object if cancelled => it's OK
    match client.next_timeout(Some(cfg.read_error_timeout.to_duration())) {
        Err(BootstrapError::TimedOut(_)) => {
            massa_trace!(
                "bootstrap.lib.bootstrap_from_server: No error sent at connection",
                {}
            );
        }
        Err(e) => return Err(e),
        Ok(BootstrapServerMessage::BootstrapError { error: err }) => {
            return Err(BootstrapError::ReceivedError(err))
        }
        Ok(msg) => return Err(BootstrapError::UnexpectedServerMessage(msg)),
    };

    // handshake
    let send_time_uncompensated = MassaTime::now();
    // client.handshake() is not cancel-safe but we drop the whole client object if cancelled => it's OK
    client.handshake(our_version)?;

    // compute ping
    let ping = MassaTime::now().saturating_sub(send_time_uncompensated);
    if ping > cfg.max_ping {
        return Err(BootstrapError::GeneralError(
            "bootstrap ping too high".into(),
        ));
    }

    // First, clock and version.
    // client.next() is not cancel-safe but we drop the whole client object if cancelled => it's OK
    let server_time = match client.next_timeout(Some(cfg.read_timeout.into())) {
        Err(e) => return Err(e),
        Ok(BootstrapServerMessage::BootstrapTime {
            server_time,
            version,
        }) => {
            if !our_version.is_compatible(&version) {
                return Err(BootstrapError::IncompatibleVersionError(format!(
                    "remote is running incompatible version: {} (local node version: {})",
                    version, our_version
                )));
            }
            server_time
        }
        Ok(BootstrapServerMessage::BootstrapError { error }) => {
            return Err(BootstrapError::ReceivedError(error))
        }
        Ok(msg) => return Err(BootstrapError::UnexpectedServerMessage(msg)),
    };

    // get the time of reception
    let recv_time = MassaTime::now();

    // compute ping
    let ping = recv_time.saturating_sub(send_time_uncompensated);
    if ping > cfg.max_ping {
        return Err(BootstrapError::GeneralError(
            "bootstrap ping too high".into(),
        ));
    }

    // compute client / server clock delta
    // div 2 is an approximation of the time it took the message to do server -> client
    // the complete ping value being client -> server -> client
    let adjusted_server_time = server_time.checked_add(ping.checked_div_u64(2)?)?;
    let clock_delta = adjusted_server_time.abs_diff(recv_time);

    // if clock delta is too high warn the user and restart bootstrap
    if clock_delta > cfg.max_clock_delta {
        warn!("client and server clocks differ too much, please check your clock");
        let message = format!(
            "client = {}, server = {}, ping = {}, max_delta = {}",
            recv_time, server_time, ping, cfg.max_clock_delta
        );
        return Err(BootstrapError::ClockError(message));
    }

    let write_timeout: std::time::Duration = cfg.write_timeout.into();
    // Loop to ask data to the server depending on the last message we sent
    loop {
        match next_bootstrap_message {
            BootstrapClientMessage::AskBootstrapPart { .. } => {
                stream_final_state_and_consensus(
                    cfg,
                    client,
                    next_bootstrap_message,
                    global_bootstrap_state,
                )?;
            }
            BootstrapClientMessage::AskBootstrapPeers => {
                let peers = match send_client_message(
                    next_bootstrap_message,
                    client,
                    write_timeout,
                    cfg.read_timeout.into(),
                    "ask bootstrap peers timed out",
                )? {
                    BootstrapServerMessage::BootstrapPeers { peers } => peers,
                    BootstrapServerMessage::BootstrapError { error } => {
                        return Err(BootstrapError::ReceivedError(error))
                    }
                    other => return Err(BootstrapError::UnexpectedServerMessage(other)),
                };
                global_bootstrap_state.peers = Some(peers);
                *next_bootstrap_message = BootstrapClientMessage::BootstrapSuccess;
            }
            BootstrapClientMessage::BootstrapSuccess => {
                client.send_timeout(next_bootstrap_message, Some(write_timeout))?;
                break;
            }
            BootstrapClientMessage::BootstrapError { error: _ } => {
                panic!("The next message to send shouldn't be BootstrapError");
            }
        };
    }
    info!("Successful bootstrap");
    Ok(())
}

/// Checks the network restart metadata announced by a bootstrap server.
///
/// The server sends `last_start_period` and `last_slot_before_downtime` together, and only once
/// per stream (on the first part, before any consensus blocks). Startup derives the network
/// downtime range and its timestamps from them (see `massa-node`), assuming they describe a
/// coherent interval, so anything else has to be rejected as a bootstrap failure rather than
/// stored.
fn check_restart_metadata(
    cfg: &BootstrapConfig,
    last_start_period: &Option<u64>,
    last_slot_before_downtime: &Option<Option<Slot>>,
) -> Result<(), BootstrapError> {
    let (last_start_period, last_slot_before_downtime) =
        match (last_start_period, last_slot_before_downtime) {
            (None, None) => return Ok(()),
            (Some(period), Some(slot)) => (*period, *slot),
            _ => {
                return Err(BootstrapError::GeneralError(
                    "the server sent only one half of the network restart metadata".into(),
                ))
            }
        };

    // the timestamp of the last start slot is computed at startup: it must not overflow
    get_block_slot_timestamp(
        cfg.thread_count,
        cfg.t0,
        cfg.genesis_timestamp,
        Slot::new(last_start_period, cfg.thread_count.saturating_sub(1)),
    )
    .map_err(|e| {
        BootstrapError::GeneralError(format!(
            "the server sent an out of range last_start_period {}: {}",
            last_start_period, e
        ))
    })?;

    let Some(last_slot_before_downtime) = last_slot_before_downtime else {
        return Ok(());
    };

    // the last slot executed before the downtime has to precede the restart
    if last_slot_before_downtime >= Slot::new(last_start_period, 0) {
        return Err(BootstrapError::GeneralError(format!(
            "the server announced a downtime starting at {} but a restart at period {}",
            last_slot_before_downtime, last_start_period
        )));
    }

    shutdown_range(cfg, last_start_period, last_slot_before_downtime).map(|_| ())
}

/// Computes the slot range of the last network shutdown, as startup does: from the slot right
/// after the last one executed before the downtime, to the slot right before the restart.
fn shutdown_range(
    cfg: &BootstrapConfig,
    last_start_period: u64,
    last_slot_before_downtime: Slot,
) -> Result<(Slot, Slot), BootstrapError> {
    let shutdown_start = last_slot_before_downtime
        .get_next_slot(cfg.thread_count)
        .map_err(|e| {
            BootstrapError::GeneralError(format!(
                "the server sent an out of range last_slot_before_downtime {}: {}",
                last_slot_before_downtime, e
            ))
        })?;
    let shutdown_end = Slot::new(last_start_period, 0)
        .get_prev_slot(cfg.thread_count)
        .map_err(|e| {
            BootstrapError::GeneralError(format!(
                "the server announced a downtime ending at period {}, which has no previous slot: {}",
                last_start_period, e
            ))
        })?;
    Ok((shutdown_start, shutdown_end))
}

fn send_client_message(
    message_to_send: &BootstrapClientMessage,
    client: &mut BootstrapClientBinder,
    write_timeout: Duration,
    read_timeout: Duration,
    error: &str,
) -> Result<BootstrapServerMessage, BootstrapError> {
    client.send_timeout(message_to_send, Some(write_timeout))?;

    client
        .next_timeout(Some(read_timeout))
        .map_err(|e| match e {
            BootstrapError::TimedOut(_) => {
                BootstrapError::TimedOut(std::io::Error::new(std::io::ErrorKind::TimedOut, error))
            }
            _ => e,
        })
}

pub(crate) fn connect_to_server(
    connector: &mut impl BSConnector,
    bootstrap_config: &BootstrapConfig,
    addr: &SocketAddr,
    pub_key: &PublicKey,
    rw_limit: Option<u64>,
) -> Result<BootstrapClientBinder, BootstrapError> {
    let socket = connector.connect_timeout(*addr, Some(bootstrap_config.connect_timeout))?;
    socket.set_nonblocking(false)?;
    Ok(BootstrapClientBinder::new(
        socket,
        *pub_key,
        bootstrap_config.into(),
        rw_limit,
    ))
}

fn filter_bootstrap_list(
    bootstrap_list: Vec<(SocketAddr, NodeId)>,
    ip_type: IpType,
) -> Vec<(SocketAddr, NodeId)> {
    let ip_filter: fn(&(SocketAddr, NodeId)) -> bool = match ip_type {
        IpType::IPv4 => |&(addr, _)| addr.is_ipv4(),
        IpType::IPv6 => |&(addr, _)| addr.is_ipv6(),
        IpType::Both => |_| true,
    };

    let prev_bootstrap_list_len = bootstrap_list.len();

    let filtered_bootstrap_list: Vec<_> = bootstrap_list.into_iter().filter(ip_filter).collect();

    let new_bootstrap_list_len = filtered_bootstrap_list.len();

    debug!(
        "Keeping {:?} bootstrap ip types. Filtered out {} bootstrap addresses out of a total of {} bootstrap servers.",
        ip_type,
        prev_bootstrap_list_len as i32 - new_bootstrap_list_len as i32,
        prev_bootstrap_list_len
    );

    filtered_bootstrap_list
}

/// Uses the cond-var pattern to handle sig-int cancellation.
/// Make sure that the passed in `interrupted` shares its Arc
/// with a sig-int handler setup.
#[allow(clippy::too_many_arguments)]
pub fn get_state(
    bootstrap_config: &BootstrapConfig,
    final_state: Arc<RwLock<dyn FinalStateController>>,
    mut connector: impl BSConnector,
    version: Version,
    genesis_timestamp: MassaTime,
    end_timestamp: Option<MassaTime>,
    restart_from_snapshot_at_period: Option<u64>,
    interrupted: Arc<(Mutex<bool>, Condvar)>,
    massa_metrics: MassaMetrics,
) -> Result<GlobalBootstrapState, BootstrapError> {
    massa_trace!("bootstrap.lib.get_state", {});

    // If we restart from a snapshot, do not bootstrap
    if restart_from_snapshot_at_period.is_some() {
        massa_trace!("bootstrap.lib.get_state.init_from_snapshot", {});
        return Ok(GlobalBootstrapState::new(final_state));
    }

    // if we are before genesis, do not bootstrap
    if MassaTime::now() < genesis_timestamp {
        massa_trace!("bootstrap.lib.get_state.init_from_scratch", {});
        // init final state
        {
            let mut final_state_guard = final_state.write();

            if !bootstrap_config.keep_ledger {
                // load ledger from initial ledger file
                final_state_guard
                    .get_ledger_mut()
                    .load_initial_ledger()
                    .map_err(|err| {
                        BootstrapError::GeneralError(format!(
                            "could not load initial ledger: {}",
                            err
                        ))
                    })?;
            }

            let slot = Slot::new(
                final_state_guard.get_last_start_period(),
                bootstrap_config.thread_count.saturating_sub(1),
            );

            // create the initial cycle of PoS cycle_history
            let mut batch = DBBatch::new();
            let mut db_versioning_batch: BTreeMap<Vec<u8>, Option<Vec<u8>>> = DBBatch::new();
            final_state_guard
                .get_pos_state_mut()
                .create_initial_cycle(&mut batch);

            // set initial execution trail hash
            final_state_guard.init_execution_trail_hash_to_batch(&mut batch);

            // load initial deferred credits
            final_state_guard
                .load_initial_deferred_credits(&mut batch)
                .map_err(|err| {
                    BootstrapError::GeneralError(format!(
                        "could not load initial deferred credits: {}",
                        err
                    ))
                })?;

            // Need to write MIP store to Db if we want to bootstrap it to others
            final_state_guard
                .get_mip_store()
                .update_batches(&mut batch, &mut db_versioning_batch, None)
                .map_err(|e| BootstrapError::GeneralError(e.to_string()))?;

            final_state_guard.get_database().write().write_batch(
                batch,
                db_versioning_batch,
                Some(slot),
            );
        }
        return Ok(GlobalBootstrapState::new(final_state));
    }

    // If the two conditions above are not verified, we need to bootstrap
    // we filter the bootstrap list to keep only the ip addresses we are compatible with
    let filtered_bootstrap_list = get_bootstrap_list_iter(bootstrap_config)?;

    let mut next_bootstrap_message: BootstrapClientMessage =
        BootstrapClientMessage::AskBootstrapPart {
            last_slot: None,
            last_state_step: StreamingStep::Started,
            last_versioning_step: StreamingStep::Started,
            last_consensus_step: StreamingStep::Started,
            send_last_start_period: true,
        };
    let mut global_bootstrap_state = GlobalBootstrapState::new(final_state);

    let limit = bootstrap_config.rate_limit;
    loop {
        // check for interruption
        if *interrupted
            .0
            .lock()
            .expect("double-lock on interrupt-mutex")
        {
            return Err(BootstrapError::Interrupted(
                "Sig INT received while getting state".to_string(),
            ));
        }
        for (addr, node_id) in filtered_bootstrap_list.iter() {
            if let Some(end) = end_timestamp {
                if MassaTime::now() > end {
                    panic!("This episode has come to an end, please get the latest testnet node version to continue");
                }
            }
            info!("Start bootstrapping from {}", addr);
            let conn = connect_to_server(
                &mut connector,
                bootstrap_config,
                addr,
                &node_id.get_public_key(),
                Some(limit),
            );
            match conn {
                Ok(mut client) => {
                    massa_metrics.inc_bootstrap_counter();
                    let bs = bootstrap_from_server(
                        bootstrap_config,
                        &mut client,
                        &mut next_bootstrap_message,
                        &mut global_bootstrap_state,
                        version,
                    );
                    // cancellable
                    match bs {
                        Err(BootstrapError::ReceivedError(error)) => {
                            warn!("Error received from bootstrap server: {}", error)
                        }
                        Err(e) => {
                            warn!("Error while bootstrapping: {}", &e);
                            // We allow unused result because we don't care if an error is thrown when sending the error message to the server we will close the socket anyway.
                            let _ = client.send_timeout(
                                &BootstrapClientMessage::BootstrapError {
                                    error: e.to_string(),
                                },
                                Some(bootstrap_config.write_error_timeout.into()),
                            );
                        }
                        Ok(()) => return Ok(global_bootstrap_state),
                    }
                }
                Err(e) => {
                    warn!("Error while connecting to bootstrap server: {}", e);
                }
            };

            info!("Bootstrap from server {} failed. Your node will try to bootstrap from another server in {}.", addr, format_duration(bootstrap_config.retry_delay.to_duration()).to_string());

            // Before, we would use a simple sleep(...), and that was fine
            // in a cancellable async context: the runtime could
            // catch the interrupt signal, and just cancel this thread:
            //
            // let state = tokio::select!{
            //    /* detect interrupt */ => /* return, cancelling the async get_state */
            //    get_state(...) => well, we got the state, and it didn't have to worry about interrupts
            // };
            //
            // Without an external system to preempt this context, we use a condvar to manage the sleep.
            //
            // Condvar::wait is basically std::thread::sleep(/* until some magic happens */)
            // Condvar::wait_timeout(..., duration) is much the same, but for a max-len of `duration`
            //
            // The _magic_ happens when, somewhere else, a clone of the Arc<(Mutex<bool>, Condvar)>\
            // calls Condvar::notify_[one | all], which prompts this thread to wake up. Assuming that
            // the mutex-wrapped variable has been set appropriately before the notify, this thread
            let int_sig = interrupted
                .0
                .lock()
                .expect("double-lock() on interrupted signal mutex");
            let wake = interrupted
                .1
                .wait_timeout(int_sig, bootstrap_config.retry_delay.to_duration())
                .expect("interrupt signal mutex poisoned");
            if *wake.0 {
                return Err(BootstrapError::Interrupted(
                    "Sig INT during bootstrap retry-wait".to_string(),
                ));
            }
        }
    }
}

fn get_bootstrap_list_iter(
    bootstrap_config: &BootstrapConfig,
) -> Result<Vec<(SocketAddr, NodeId)>, BootstrapError> {
    let mut filtered_bootstrap_list = filter_bootstrap_list(
        bootstrap_config.bootstrap_list.clone(),
        bootstrap_config.bootstrap_protocol,
    );

    // we are after genesis => bootstrap
    massa_trace!("bootstrap.lib.get_state.init_from_others", {});
    if filtered_bootstrap_list.is_empty() {
        return Err(BootstrapError::GeneralError(
            "no bootstrap nodes found in list".into(),
        ));
    }

    // we shuffle the list
    filtered_bootstrap_list.shuffle(&mut StdRng::from_entropy());

    // we remove the duplicated node ids (if a bootstrap server appears both with its IPv4 and IPv6 address)
    let mut unique_node_ids: HashSet<NodeId> = HashSet::new();
    filtered_bootstrap_list.retain(|e| unique_node_ids.insert(e.1));
    Ok(filtered_bootstrap_list)
}

fn warn_user_about_versioning_updates(updated: Vec<MipInfo>, added: BTreeMap<MipInfo, MipState>) {
    if !added.is_empty() {
        for (mip_info, mip_state) in added.iter() {
            let now = MassaTime::now();
            match mip_state.state_at(
                now,
                mip_info.start,
                mip_info.timeout,
                mip_info.activation_delay,
            ) {
                Ok(st_id) => {
                    if st_id == ComponentStateTypeId::LockedIn {
                        // A new MipInfo @ state locked_in - we need to urge the user to update
                        warn!(
                            "A new MIP has been locked in: {}, version: {}",
                            mip_info.name, mip_info.version
                        );
                        // Safe to unwrap here (only panic if not LockedIn)
                        let activation_at = mip_state.activation_at(mip_info).unwrap();

                        warn!(
                            "Please update your Massa node before: {}",
                            activation_at.format_instant()
                        );
                    } else if st_id == ComponentStateTypeId::Active {
                        // A new MipInfo @ state active - we are not compatible anymore
                        warn!(
                            "A new MIP has become active {:?}, version: {:?}",
                            mip_info.name, mip_info.version
                        );
                        panic!(
                            "Please update your Massa node to support MIP version {} ({})",
                            mip_info.version, mip_info.name
                        );
                    } else if st_id == ComponentStateTypeId::Defined {
                        // a new MipInfo @ state defined or started (or failed / error)
                        // warn the user to update its node
                        warn!(
                            "A new MIP has been defined: {}, version: {}",
                            mip_info.name, mip_info.version
                        );
                        debug!("MIP state: {:?}", mip_state);

                        warn!("Please update your node between: {} and {} if you want to support this update", mip_info.start.format_instant(), mip_info.timeout.format_instant());
                    } else {
                        // a new MipInfo @ state defined or started (or failed / error)
                        // warn the user to update its node
                        warn!(
                            "A new MIP has been received: {}, version: {}",
                            mip_info.name, mip_info.version
                        );
                        debug!("MIP state: {:?}", mip_state);
                        warn!("Please update your Massa node to support it");
                    }
                }
                Err(StateAtError::Unpredictable) => {
                    warn!(
                        "A new MIP has started: {}, version: {}",
                        mip_info.name, mip_info.version
                    );
                    debug!("MIP state: {:?}", mip_state);

                    warn!("Please update your node between: {} and {} if you want to support this update", mip_info.start.format_instant(), mip_info.timeout.format_instant());
                }
                Err(e) => {
                    // Should never happen
                    panic!(
                        "Unable to get state at {} of mip info: {:?}, error: {}",
                        now, mip_info, e
                    )
                }
            }
        }
    }

    debug!("MIP store got {} MIP updated from bootstrap", updated.len());
}

#[cfg(test)]
mod restart_metadata_tests {
    use super::*;
    use crate::tests::tools::get_bootstrap_config;
    use massa_signature::KeyPair;

    fn config() -> BootstrapConfig {
        get_bootstrap_config(NodeId::new(KeyPair::generate(0).unwrap().get_public_key()))
    }

    #[test]
    fn accepts_plausible_restart_metadata() {
        let cfg = config();
        let tc = cfg.thread_count;
        for (last_start_period, last_slot_before_downtime) in [
            // a server that has nothing to say about a restart
            (None, None),
            // a network that never restarted
            (Some(0), Some(None)),
            // a restart leaving no idle slot behind
            (Some(1), Some(Some(Slot::new(0, tc - 1)))),
            // a restart after an actual downtime
            (Some(100), Some(Some(Slot::new(42, 3)))),
        ] {
            check_restart_metadata(&cfg, &last_start_period, &last_slot_before_downtime).unwrap();
        }
    }

    #[test]
    fn rejects_impossible_restart_metadata() {
        let cfg = config();
        for (last_start_period, last_slot_before_downtime) in [
            // only one half of the metadata
            (Some(2), None),
            (None, Some(Some(Slot::new(1, 0)))),
            // period 0 has no previous slot, the downtime cannot end before the restart
            (Some(0), Some(Some(Slot::new(0, 0)))),
            // the downtime starts after the restart
            (Some(1), Some(Some(Slot::new(5, 0)))),
            // the restart timestamp does not fit in a `MassaTime`
            (Some(u64::MAX), Some(None)),
        ] {
            check_restart_metadata(&cfg, &last_start_period, &last_slot_before_downtime)
                .expect_err(&format!(
                    "accepted {:?} / {:?}",
                    last_start_period, last_slot_before_downtime
                ));
        }
    }
}
