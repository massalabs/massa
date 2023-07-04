use humantime::format_duration;
use massa_db_exports::DBBatch;
use massa_final_state::{FinalState, FinalStateError};
use massa_logging::massa_trace;
use massa_models::{node::NodeId, slot::Slot, streaming_step::StreamingStep, version::Version};
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
fn stream_final_state_and_consensus(
    cfg: &BootstrapConfig,
    client: &mut BootstrapClientBinder,
    next_bootstrap_message: &mut BootstrapClientMessage,
    global_bootstrap_state: &mut GlobalBootstrapState,
) -> Result<(), BootstrapError> {
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

                    // We only need to receive the initial_state once
                    if let Some(last_start_period) = last_start_period {
                        write_final_state.last_start_period = last_start_period;
                    }
                    if let Some(last_slot_before_downtime) = last_slot_before_downtime {
                        write_final_state.last_slot_before_downtime = last_slot_before_downtime;
                    }

                    let (last_state_step, last_versioning_step) = write_final_state
                        .db
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
                    // Set next bootstrap message
                    *next_bootstrap_message = BootstrapClientMessage::AskBootstrapPeers;

                    // Update MIP store by reading from the disk
                    let mut guard = global_bootstrap_state.final_state.write();
                    let db = guard.db.clone();
                    let (updated, added) = guard
                        .mip_store
                        .extend_from_db(db)
                        .map_err(|e| BootstrapError::from(FinalStateError::from(e)))?;

                    warn_user_about_versioning_updates(updated, added);

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
fn bootstrap_from_server(
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
    let send_time_uncompensated = MassaTime::now()?;
    // client.handshake() is not cancel-safe but we drop the whole client object if cancelled => it's OK
    client.handshake(our_version)?;

    // compute ping
    let ping = MassaTime::now()?.saturating_sub(send_time_uncompensated);
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
    let recv_time = MassaTime::now()?;

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

fn connect_to_server(
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
    final_state: Arc<RwLock<FinalState>>,
    mut connector: impl BSConnector,
    version: Version,
    genesis_timestamp: MassaTime,
    end_timestamp: Option<MassaTime>,
    restart_from_snapshot_at_period: Option<u64>,
    interupted: Arc<(Mutex<bool>, Condvar)>,
) -> Result<GlobalBootstrapState, BootstrapError> {
    massa_trace!("bootstrap.lib.get_state", {});

    // If we restart from a snapshot, do not bootstrap
    if restart_from_snapshot_at_period.is_some() {
        massa_trace!("bootstrap.lib.get_state.init_from_snapshot", {});
        return Ok(GlobalBootstrapState::new(final_state));
    }

    // if we are before genesis, do not bootstrap
    if MassaTime::now()? < genesis_timestamp {
        massa_trace!("bootstrap.lib.get_state.init_from_scratch", {});
        // init final state
        {
            let mut final_state_guard = final_state.write();

            if !bootstrap_config.keep_ledger {
                // load ledger from initial ledger file
                final_state_guard
                    .ledger
                    .load_initial_ledger()
                    .map_err(|err| {
                        BootstrapError::GeneralError(format!(
                            "could not load initial ledger: {}",
                            err
                        ))
                    })?;
            }

            // create the initial cycle of PoS cycle_history
            let mut batch = DBBatch::new();
            let mut db_versioning_batch = DBBatch::new();
            final_state_guard.pos_state.create_initial_cycle(&mut batch);

            let slot = Slot::new(
                final_state_guard.last_start_period,
                bootstrap_config.thread_count.saturating_sub(1),
            );

            // Need to write MIP store to Db if we want to bootstrap it to others
            final_state_guard
                .mip_store
                .update_batches(&mut batch, &mut db_versioning_batch, None)
                .map_err(|e| BootstrapError::GeneralError(e.to_string()))?;

            final_state_guard
                .db
                .write()
                .write_batch(batch, db_versioning_batch, Some(slot));
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

    let limit = bootstrap_config.max_bytes_read_write;
    loop {
        // check for interuption
        if *interupted.0.lock().expect("double-lock on interupt-mutex") {
            return Err(BootstrapError::Interupted(
                "Sig INT received while getting state".to_string(),
            ));
        }
        for (addr, node_id) in filtered_bootstrap_list.iter() {
            if let Some(end) = end_timestamp {
                if MassaTime::now().expect("could not get now time") > end {
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
            // catch the interupt signal, and just cancel this thread:
            //
            // let state = tokio::select!{
            //    /* detect interupt */ => /* return, cancelling the async get_state */
            //    get_state(...) => well, we got the state, and it didn't have to worry about interupts
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
            let int_sig = interupted
                .0
                .lock()
                .expect("double-lock() on interupted signal mutex");
            let wake = interupted
                .1
                .wait_timeout(int_sig, bootstrap_config.retry_delay.to_duration())
                .expect("interupt signal mutex poisoned");
            if *wake.0 {
                return Err(BootstrapError::Interupted(
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
            let now = MassaTime::now().expect("Cannot get current time");
            match mip_state.state_at(now, mip_info.start, mip_info.timeout) {
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
