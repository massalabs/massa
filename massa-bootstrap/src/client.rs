use humantime::format_duration;
use std::{collections::HashSet, net::SocketAddr, sync::Arc, time::Duration};

use massa_final_state::FinalState;
use massa_logging::massa_trace;
use massa_models::{node::NodeId, streaming_step::StreamingStep, version::Version};
use massa_signature::PublicKey;
use massa_time::MassaTime;
use parking_lot::RwLock;
use rand::{
    prelude::{SliceRandom, StdRng},
    SeedableRng,
};
use tokio::time::sleep;
use tracing::{debug, info, warn};

use crate::{
    client_binder::BootstrapClientBinder,
    error::BootstrapError,
    messages::{BootstrapClientMessage, BootstrapServerMessage},
    settings::IpType,
    BootstrapConfig, Establisher, GlobalBootstrapState,
};

/// This function will send the starting point to receive a stream of the ledger and will receive and process each part until receive a `BootstrapServerMessage::FinalStateFinished` message from the server.
/// `next_bootstrap_message` passed as parameter must be `BootstrapClientMessage::AskFinalStatePart` enum variant.
/// `next_bootstrap_message` will be updated after receiving each part so that in case of connection lost we can restart from the last message we processed.
async fn stream_final_state_and_consensus(
    cfg: &BootstrapConfig,
    client: &mut BootstrapClientBinder,
    next_bootstrap_message: &mut BootstrapClientMessage,
    global_bootstrap_state: &mut GlobalBootstrapState,
) -> Result<(), BootstrapError> {
    if let BootstrapClientMessage::AskBootstrapPart { .. } = &next_bootstrap_message {
        match tokio::time::timeout(
            cfg.write_timeout.into(),
            client.send(next_bootstrap_message),
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
        loop {
            let msg = match tokio::time::timeout(cfg.read_timeout.into(), client.next()).await {
                Err(_) => {
                    return Err(std::io::Error::new(
                        std::io::ErrorKind::TimedOut,
                        "final state bootstrap read timed out",
                    )
                    .into());
                }
                Ok(Err(e)) => return Err(e),
                Ok(Ok(msg)) => msg,
            };
            match msg {
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
                } => {
                    // Set final state
                    let mut write_final_state = global_bootstrap_state.final_state.write();
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
                        write_final_state
                            .ledger
                            .apply_changes(changes.ledger_changes.clone(), *changes_slot);
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
                    };
                    let mut write_final_state = global_bootstrap_state.final_state.write();
                    write_final_state.reset();
                    return Err(BootstrapError::GeneralError(String::from("Slot too old")));
                }
                BootstrapServerMessage::BootstrapError { error } => {
                    return Err(std::io::Error::new(std::io::ErrorKind::InvalidData, error).into())
                }
                _ => {
                    return Err(std::io::Error::new(
                        std::io::ErrorKind::InvalidData,
                        "unexpected message",
                    )
                    .into())
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
async fn bootstrap_from_server(
    cfg: &BootstrapConfig,
    client: &mut BootstrapClientBinder,
    next_bootstrap_message: &mut BootstrapClientMessage,
    global_bootstrap_state: &mut GlobalBootstrapState,
    our_version: Version,
) -> Result<(), BootstrapError> {
    massa_trace!("bootstrap.lib.bootstrap_from_server", {});

    // read error (if sent by the server)
    // client.next() is not cancel-safe but we drop the whole client object if cancelled => it's OK
    match tokio::time::timeout(cfg.read_error_timeout.into(), client.next()).await {
        Err(_) => {
            massa_trace!(
                "bootstrap.lib.bootstrap_from_server: No error sent at connection",
                {}
            );
        }
        Ok(Err(e)) => return Err(e),
        Ok(Ok(BootstrapServerMessage::BootstrapError { error: err })) => {
            return Err(BootstrapError::ReceivedError(err))
        }
        Ok(Ok(msg)) => return Err(BootstrapError::UnexpectedServerMessage(msg)),
    };

    // handshake
    let send_time_uncompensated = MassaTime::now()?;
    // client.handshake() is not cancel-safe but we drop the whole client object if cancelled => it's OK
    match tokio::time::timeout(cfg.write_timeout.into(), client.handshake(our_version)).await {
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
        Ok(Ok(BootstrapServerMessage::BootstrapTime {
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
        Ok(Ok(BootstrapServerMessage::BootstrapError { error })) => {
            return Err(BootstrapError::ReceivedError(error))
        }
        Ok(Ok(msg)) => return Err(BootstrapError::UnexpectedServerMessage(msg)),
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
                )
                .await?;
            }
            BootstrapClientMessage::AskBootstrapPeers => {
                let peers = match send_client_message(
                    next_bootstrap_message,
                    client,
                    write_timeout,
                    cfg.read_timeout.into(),
                    "ask bootstrap peers timed out",
                )
                .await?
                {
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
                match tokio::time::timeout(write_timeout, client.send(next_bootstrap_message)).await
                {
                    Err(_) => Err(std::io::Error::new(
                        std::io::ErrorKind::TimedOut,
                        "send bootstrap success timed out",
                    )
                    .into()),
                    Ok(Err(e)) => Err(e),
                    Ok(Ok(_)) => Ok(()),
                }?;
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

async fn send_client_message(
    message_to_send: &BootstrapClientMessage,
    client: &mut BootstrapClientBinder,
    write_timeout: Duration,
    read_timeout: Duration,
    error: &str,
) -> Result<BootstrapServerMessage, BootstrapError> {
    match tokio::time::timeout(write_timeout, client.send(message_to_send)).await {
        Err(_) => Err(std::io::Error::new(std::io::ErrorKind::TimedOut, error).into()),
        Ok(Err(e)) => Err(e),
        Ok(Ok(_)) => Ok(()),
    }?;
    match tokio::time::timeout(read_timeout, client.next()).await {
        Err(_) => Err(std::io::Error::new(std::io::ErrorKind::TimedOut, error).into()),
        Ok(Err(e)) => Err(e),
        Ok(Ok(msg)) => Ok(msg),
    }
}

async fn connect_to_server(
    establisher: &mut Establisher,
    bootstrap_config: &BootstrapConfig,
    addr: &SocketAddr,
    pub_key: &PublicKey,
) -> Result<BootstrapClientBinder, BootstrapError> {
    // connect
    let mut connector = establisher
        .get_connector(bootstrap_config.connect_timeout)
        .await?; // cancellable
    let socket = connector.connect(*addr).await?; // cancellable
    Ok(BootstrapClientBinder::new(
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
        // init final state
        {
            let mut final_state_guard = final_state.write();
            // load ledger from initial ledger file
            final_state_guard
                .ledger
                .load_initial_ledger()
                .map_err(|err| {
                    BootstrapError::GeneralError(format!("could not load initial ledger: {}", err))
                })?;
            // create the initial cycle of PoS cycle_history
            final_state_guard.pos_state.create_initial_cycle();
        }
        return Ok(GlobalBootstrapState::new(final_state));
    }

    // we filter the bootstrap list to keep only the ip addresses we are compatible with
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

    let mut next_bootstrap_message: BootstrapClientMessage =
        BootstrapClientMessage::AskBootstrapPart {
            last_slot: None,
            last_ledger_step: StreamingStep::Started,
            last_pool_step: StreamingStep::Started,
            last_cycle_step: StreamingStep::Started,
            last_credits_step: StreamingStep::Started,
            last_ops_step: StreamingStep::Started,
            last_consensus_step: StreamingStep::Started,
        };
    let mut global_bootstrap_state = GlobalBootstrapState::new(final_state.clone());

    loop {
        for (addr, node_id) in filtered_bootstrap_list.iter() {
            if let Some(end) = end_timestamp {
                if MassaTime::now().expect("could not get now time") > end {
                    panic!("This episode has come to an end, please get the latest testnet node version to continue");
                }
            }
            info!("Start bootstrapping from {}", addr);
            match connect_to_server(
                &mut establisher,
                bootstrap_config,
                addr,
                &node_id.get_public_key(),
            )
            .await
            {
                Ok(mut client) => {
                    match bootstrap_from_server(bootstrap_config, &mut client, &mut next_bootstrap_message, &mut global_bootstrap_state,version)
                    .await  // cancellable
                    {
                        Err(BootstrapError::ReceivedError(error)) => warn!("Error received from bootstrap server: {}", error),
                        Err(e) => {
                            warn!("Error while bootstrapping: {}", e);
                            // We allow unused result because we don't care if an error is thrown when sending the error message to the server we will close the socket anyway.
                            let _ = tokio::time::timeout(bootstrap_config.write_error_timeout.into(), client.send(&BootstrapClientMessage::BootstrapError { error: e.to_string() })).await;
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
            sleep(bootstrap_config.retry_delay.into()).await;
        }
    }
}
