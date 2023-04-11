use humantime::format_duration;
use std::{collections::HashSet, net::SocketAddr, sync::Arc, time::Duration};

use massa_final_state::FinalState;
use massa_logging::massa_trace;
use massa_models::{node::NodeId, streaming_step::StreamingStep, version::Version};
use massa_signature::PublicKey;
use massa_time::MassaTime;
use massa_versioning_worker::versioning::MipStore;
use parking_lot::RwLock;
use rand::{
    prelude::{SliceRandom, StdRng},
    SeedableRng,
};
use tracing::{debug, info, warn};

use crate::{
    client_binder::{BootstrapClientBinder, NonHSClientBinder},
    error::BootstrapError,
    establisher::BSConnector,
    messages::{BootstrapClientMessage, BootstrapServerMessage},
    settings::IpType,
    BootstrapConfig, GlobalBootstrapState,
};

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
                    ledger_part,
                    async_pool_part,
                    pos_cycle_part,
                    pos_credits_part,
                    exec_ops_part,
                    final_state_changes,
                    consensus_part,
                    consensus_outdated_ids,
                    last_start_period,
                } => {
                    // Set final state
                    let mut write_final_state = global_bootstrap_state.final_state.write();

                    // We only need to receive the initial_state once
                    if let Some(last_start_period) = last_start_period {
                        write_final_state.last_start_period = last_start_period;
                    }

                    let last_ledger_step = write_final_state.ledger.set_ledger_part(ledger_part)?;
                    let last_pool_step =
                        write_final_state.async_pool.set_pool_part(async_pool_part);
                    let last_cycle_step = write_final_state
                        .pos_state
                        .set_cycle_history_part(pos_cycle_part);
                    let last_credits_step = write_final_state
                        .pos_state
                        .set_deferred_credits_part(pos_credits_part);
                    let last_ops_step = write_final_state
                        .executed_ops
                        .set_executed_ops_part(exec_ops_part);
                    for (changes_slot, changes) in final_state_changes.iter() {
                        write_final_state.ledger.apply_changes(
                            changes.ledger_changes.clone(),
                            *changes_slot,
                            None,
                        );
                        write_final_state
                            .async_pool
                            .apply_changes_unchecked(&changes.async_pool_changes);
                        if !changes.pos_changes.is_empty() {
                            write_final_state.pos_state.apply_changes(
                                changes.pos_changes.clone(),
                                *changes_slot,
                                false,
                            )?;
                        }
                        if !changes.executed_ops_changes.is_empty() {
                            write_final_state
                                .executed_ops
                                .apply_changes(changes.executed_ops_changes.clone(), *changes_slot);
                        }
                    }
                    write_final_state.slot = slot;

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
                        last_ledger_step,
                        last_pool_step,
                        last_cycle_step,
                        last_credits_step,
                        last_ops_step,
                        last_consensus_step,
                        send_last_start_period: false,
                    };

                    // Logs for an easier diagnostic if needed
                    debug!(
                        "client final state bootstrap cursors: {:?}",
                        next_bootstrap_message
                    );
                    debug!(
                        "client final state slot changes length: {}",
                        final_state_changes.len()
                    );
                }
                BootstrapServerMessage::BootstrapFinished => {
                    info!("State bootstrap complete");
                    // Set next bootstrap message
                    *next_bootstrap_message = BootstrapClientMessage::AskBootstrapPeers;
                    return Ok(());
                }
                BootstrapServerMessage::SlotTooOld => {
                    info!("Slot is too old retry bootstrap from scratch");
                    *next_bootstrap_message = BootstrapClientMessage::AskBootstrapPart {
                        last_slot: None,
                        last_ledger_step: StreamingStep::Started,
                        last_pool_step: StreamingStep::Started,
                        last_cycle_step: StreamingStep::Started,
                        last_credits_step: StreamingStep::Started,
                        last_ops_step: StreamingStep::Started,
                        last_consensus_step: StreamingStep::Started,
                        send_last_start_period: true,
                    };
                    let mut write_final_state = global_bootstrap_state.final_state.write();
                    write_final_state.reset();
                    return Err(BootstrapError::GeneralError(String::from("Slot too old")));
                }
                // At this point, we have succesfully received the next message from the server, and it's an error-message String
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
    client: NonHSClientBinder,
    next_bootstrap_message: &mut BootstrapClientMessage,
    global_bootstrap_state: &mut GlobalBootstrapState,
    our_version: Version,
) -> Result<(), (Option<BootstrapClientBinder>, BootstrapError)> {
    massa_trace!("bootstrap.lib.bootstrap_from_server", {});

    // handshake
    let send_time_uncompensated = MassaTime::now().map_err(|e| (None, e.into()))?;

    let (mut client, ping) = client
        .handshake(our_version, cfg.read_error_timeout.to_duration())
        .map_err(|e| (None, e.into()))?;

    if ping > cfg.max_ping {
        return Err((
            Some(client),
            BootstrapError::GeneralError("bootstrap ping too high".into()),
        ));
    }

    // First, clock and version.
    // client.next_timeout() is not cancel-safe but we drop the whole client object if cancelled => it's OK
    let server_time = match client.next_timeout(Some(cfg.read_timeout.into())) {
        Err(e) => return Err((Some(client), e)),
        Ok(BootstrapServerMessage::BootstrapTime {
            server_time,
            version,
        }) => {
            if !our_version.is_compatible(&version) {
                return Err((
                    Some(client),
                    BootstrapError::IncompatibleVersionError(format!(
                        "remote is running incompatible version: {} (local node version: {})",
                        version, our_version
                    )),
                ));
            }
            server_time
        }
        Ok(BootstrapServerMessage::BootstrapError { error }) => {
            return Err((Some(client), BootstrapError::ReceivedError(error)))
        }
        Ok(msg) => return Err((Some(client), BootstrapError::UnexpectedServerMessage(msg))),
    };

    // get the time of reception
    let recv_time = match MassaTime::now() {
        Ok(now) => now,
        Err(e) => return Err((Some(client), e.into())),
    };

    // compute ping
    let ping = recv_time.saturating_sub(send_time_uncompensated);
    if ping > cfg.max_ping {
        return Err((
            Some(client),
            BootstrapError::GeneralError("bootstrap ping too high".into()),
        ));
    }

    // compute client / server clock delta
    // div 2 is an approximation of the time it took the message to do server -> client
    // the complete ping value being client -> server -> client
    let adjusted_server_time = match server_time.checked_add(match ping.checked_div_u64(2) {
        Ok(adjusted_server_time) => adjusted_server_time,
        Err(e) => return Err((Some(client), e.into())),
    }) {
        Ok(adjusted_server_time) => adjusted_server_time,
        Err(e) => return Err((Some(client), e.into())),
    };
    let clock_delta = adjusted_server_time.abs_diff(recv_time);

    // if clock delta is too high warn the user and restart bootstrap
    if clock_delta > cfg.max_clock_delta {
        warn!("client and server clocks differ too much, please check your clock");
        let message = format!(
            "client = {}, server = {}, ping = {}, max_delta = {}",
            recv_time, server_time, ping, cfg.max_clock_delta
        );
        return Err((Some(client), BootstrapError::ClockError(message)));
    }

    let write_timeout: std::time::Duration = cfg.write_timeout.into();
    // Loop to ask data to the server depending on the last message we sent
    loop {
        match next_bootstrap_message {
            BootstrapClientMessage::AskBootstrapPart { .. } => {
                match stream_final_state_and_consensus(
                    cfg,
                    &mut client,
                    next_bootstrap_message,
                    global_bootstrap_state,
                ) {
                    Ok(_) => (),
                    Err(e) => return Err((Some(client), e.into())),
                }
            }
            BootstrapClientMessage::AskBootstrapPeers => {
                let peers = match match send_client_message(
                    next_bootstrap_message,
                    &mut client,
                    write_timeout,
                    cfg.read_timeout.into(),
                    "ask bootstrap peers timed out",
                ) {
                    Ok(peers) => peers,
                    Err(e) => return Err((Some(client), e.into())),
                } {
                    BootstrapServerMessage::BootstrapPeers { peers } => peers,
                    BootstrapServerMessage::BootstrapError { error } => {
                        return Err((Some(client), BootstrapError::ReceivedError(error)))
                    }
                    other => {
                        return Err((Some(client), BootstrapError::UnexpectedServerMessage(other)))
                    }
                };
                global_bootstrap_state.peers = Some(peers);
                *next_bootstrap_message = BootstrapClientMessage::AskBootstrapMipStore;
            }
            BootstrapClientMessage::AskBootstrapMipStore => {
                let mip_store_raw = match send_client_message(
                    next_bootstrap_message,
                    &mut client,
                    write_timeout,
                    cfg.read_timeout.into(),
                    "ask bootstrap versioning store timed out",
                ) {
                    Err(e) => return Err((Some(client), e.into())),
                    Ok(msg) => match msg {
                        BootstrapServerMessage::BootstrapMipStore { store: store_raw } => store_raw,
                        BootstrapServerMessage::BootstrapError { error } => {
                            return Err((Some(client), BootstrapError::ReceivedError(error)));
                        }
                        other => {
                            return Err((
                                Some(client),
                                BootstrapError::UnexpectedServerMessage(other),
                            ));
                        }
                    },
                };

                global_bootstrap_state.mip_store =
                    Some(MipStore(Arc::new(RwLock::new(mip_store_raw))));
                *next_bootstrap_message = BootstrapClientMessage::BootstrapSuccess;
            }
            BootstrapClientMessage::BootstrapSuccess => {
                client
                    .send_timeout(next_bootstrap_message, Some(write_timeout))
                    .map_err(|e| (Some(client), e.into()))?;
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
) -> Result<NonHSClientBinder, BootstrapError> {
    let socket = connector.connect_timeout(*addr, Some(bootstrap_config.connect_timeout))?;
    socket.set_nonblocking(false)?;
    Ok(NonHSClientBinder::new(
        socket,
        *pub_key,
        bootstrap_config.into(),
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
        "Keeping {:?} bootstrap ips. Filtered out {} bootstrap addresses out of a total of {} bootstrap servers.",
        ip_type,
        prev_bootstrap_list_len as i32 - new_bootstrap_list_len as i32,
        prev_bootstrap_list_len
    );

    filtered_bootstrap_list
}

/// Gets the state from a bootstrap server
/// needs to be CANCELLABLE
pub async fn get_state(
    bootstrap_config: &BootstrapConfig,
    final_state: Arc<RwLock<FinalState>>,
    mut connector: impl BSConnector,
    version: Version,
    genesis_timestamp: MassaTime,
    end_timestamp: Option<MassaTime>,
    restart_from_snapshot_at_period: Option<u64>,
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
            final_state_guard.pos_state.create_initial_cycle();
        }
        return Ok(GlobalBootstrapState::new(final_state));
    }

    // If the two conditions above are not verified, we need to bootstrap
    // we filter the bootstrap list to keep only the ip addresses we are compatible with
    let filtered_bootstrap_list = get_bootstrap_list_iter(bootstrap_config)?;

    let mut next_bootstrap_message: BootstrapClientMessage =
        BootstrapClientMessage::AskBootstrapPart {
            last_slot: None,
            last_ledger_step: StreamingStep::Started,
            last_pool_step: StreamingStep::Started,
            last_cycle_step: StreamingStep::Started,
            last_credits_step: StreamingStep::Started,
            last_ops_step: StreamingStep::Started,
            last_consensus_step: StreamingStep::Started,
            send_last_start_period: true,
        };
    let mut global_bootstrap_state = GlobalBootstrapState::new(final_state);

    loop {
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
            );
            match conn {
                Ok(client) => {
                    match bootstrap_from_server(bootstrap_config, client, &mut next_bootstrap_message, &mut global_bootstrap_state,version)
                      // cancellable
                    {
                        Err((Some(_client), BootstrapError::ReceivedError(error))) => warn!("Error received from bootstrap server: {}", error),
                        Err((Some(mut client), e)) => {
                            warn!("Error while bootstrapping: {}", e);
                            // We allow unused result because we don't care if an error is thrown when sending the error message to the server we will close the socket anyway.
                            let _ = client.send_timeout(&BootstrapClientMessage::BootstrapError { error: e.to_string() }, Some(bootstrap_config.write_error_timeout.into()));
                        }
                        Err((None, e)) => {
                            warn!("Error while bootstrapping: {}", e);
                        }
                        Ok(()) => {
                            return Ok(global_bootstrap_state)
                        }
                    }
                }
                Err(e) => {
                    warn!("Error while connecting to bootstrap server: {}", e);
                }
            };

            info!("Bootstrap from server {} failed. Your node will try to bootstrap from another server in {}.", addr, format_duration(bootstrap_config.retry_delay.to_duration()).to_string());
            std::thread::sleep(bootstrap_config.retry_delay.into());
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
