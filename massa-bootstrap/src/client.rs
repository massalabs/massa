use std::{net::SocketAddr, sync::Arc, time::Duration};

use massa_final_state::FinalState;
use massa_ledger::{Applicable, LedgerCursor};
use massa_logging::massa_trace;
use massa_models::Version;
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
    messages::{BootstrapMessageClient, BootstrapMessageServer},
    BootstrapSettings, Establisher, GlobalBootstrapState,
};

async fn stream_ledger(
    cfg: &BootstrapSettings,
    client: &mut BootstrapClientBinder,
    next_message_bootstrap: &mut Option<BootstrapMessageClient>,
    global_bootstrap_state: &mut GlobalBootstrapState,
) -> Result<(), BootstrapError> {
    match tokio::time::timeout(
        cfg.write_timeout.into(),
        client.send(next_message_bootstrap.as_ref().unwrap()),
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
    let mut old_cursor: Option<LedgerCursor> = None;
    loop {
        let msg = match tokio::time::timeout(cfg.read_timeout.into(), client.next()).await {
            Err(_) => {
                println!("client: time out asking");
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
            BootstrapMessageServer::FinalStatePart {
                ledger_data,
                async_pool_part,
                slot,
                final_state_changes,
            } => {
                old_cursor = global_bootstrap_state
                    .final_state
                    .write()
                    .ledger
                    .set_ledger_part(old_cursor, ledger_data)?;
                let old_last_async_id = global_bootstrap_state
                    .final_state
                    .write()
                    .async_pool
                    .set_pool_part(async_pool_part)
                    .map(|(id, _)| *id);
                for changes in final_state_changes {
                    global_bootstrap_state
                        .final_state
                        .write()
                        .ledger
                        .apply(changes.ledger_changes);
                    global_bootstrap_state
                        .final_state
                        .write()
                        .async_pool
                        .apply_changes_unchecked(changes.async_pool_changes);
                }
                // Set new message in case of disconnection
                *next_message_bootstrap = Some(BootstrapMessageClient::AskFinalStatePart {
                    cursor: old_cursor.clone(),
                    slot: Some(slot),
                    last_async_message_id: old_last_async_id,
                });
            }
            BootstrapMessageServer::FinalStateFinished => {
                *next_message_bootstrap = Some(BootstrapMessageClient::AskBootstrapPeers);
                return Ok(());
            }
            _ => {
                return Err(std::io::Error::new(std::io::ErrorKind::TimedOut, "bad message").into())
            }
        }
    }
}

/// Gets the state from a bootstrap server (internal private function)
/// needs to be CANCELLABLE
async fn bootstrap_from_server(
    cfg: &BootstrapSettings, // TODO: should be a &'static ... see #1848
    client: &mut BootstrapClientBinder,
    next_message_bootstrap: &mut Option<BootstrapMessageClient>,
    global_bootstrap_state: &mut GlobalBootstrapState,
    our_version: Version,
) -> Result<(), BootstrapError> {
    massa_trace!("bootstrap.lib.bootstrap_from_server", {});

    // read error (if sent by the server)
    // client.next() is not cancel-safe but we drop the whole client object if cancelled => it's OK
    match tokio::time::timeout(cfg.read_error_timeout.into(), client.next()).await {
        Err(_) => {
            massa_trace!("bootstrap.lib.bootstrap_from_server: No error sent at connection", {});
        }
        Ok(Err(e)) => return Err(e),
        Ok(Ok(BootstrapMessageServer::BootstrapError{error: _})) => {
            return Err(BootstrapError::ReceivedError(
                "Bootstrap cancelled on this server because there is no slots available on this server. Will try to bootstrap to another node soon.".to_string()
            ))
        }
        Ok(Ok(msg)) => return Err(BootstrapError::UnexpectedMessageServer(msg))
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
        Ok(Ok(BootstrapMessageServer::BootstrapTime {
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
        Ok(Ok(BootstrapMessageServer::BootstrapError { error })) => {
            return Err(BootstrapError::ReceivedError(error))
        }
        Ok(Ok(msg)) => return Err(BootstrapError::UnexpectedMessageServer(msg)),
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

    global_bootstrap_state.compensation_millis = compensation_millis;

    let write_timeout: std::time::Duration = cfg.write_timeout.into();
    // Loop to ask data to the server depending on the last message we sent
    // TODO: Add ledger to the state
    loop {
        match next_message_bootstrap {
            Some(BootstrapMessageClient::AskFinalStatePart { .. }) => {
                stream_ledger(cfg, client, next_message_bootstrap, global_bootstrap_state).await?;
            }
            Some(BootstrapMessageClient::AskBootstrapPeers) => {
                let peers = match send_message_client(
                    next_message_bootstrap.as_ref().unwrap(),
                    client,
                    write_timeout,
                    cfg.read_timeout.into(),
                )
                .await?
                {
                    BootstrapMessageServer::BootstrapPeers { peers } => peers,
                    BootstrapMessageServer::BootstrapError { error } => {
                        return Err(BootstrapError::ReceivedError(error))
                    }
                    other => return Err(BootstrapError::UnexpectedMessageServer(other)),
                };
                global_bootstrap_state.peers = Some(peers);
                *next_message_bootstrap = Some(BootstrapMessageClient::AskConsensusState);
            }
            Some(BootstrapMessageClient::AskConsensusState) => {
                let state = match send_message_client(
                    next_message_bootstrap.as_ref().unwrap(),
                    client,
                    write_timeout,
                    cfg.read_timeout.into(),
                )
                .await?
                {
                    BootstrapMessageServer::ConsensusState { pos, graph } => (pos, graph),
                    BootstrapMessageServer::BootstrapError { error } => {
                        return Err(BootstrapError::ReceivedError(error))
                    }
                    other => return Err(BootstrapError::UnexpectedMessageServer(other)),
                };
                global_bootstrap_state.pos = Some(state.0);
                global_bootstrap_state.graph = Some(state.1);
                *next_message_bootstrap = None;
            }
            None => {
                if global_bootstrap_state.graph.is_none() || global_bootstrap_state.pos.is_none() {
                    *next_message_bootstrap = Some(BootstrapMessageClient::AskConsensusState);
                    continue;
                }
                if global_bootstrap_state.peers.is_none() {
                    *next_message_bootstrap = Some(BootstrapMessageClient::AskBootstrapPeers);
                    continue;
                }
                break;
            }
            Some(BootstrapMessageClient::BootstrapError { error: _ }) => {
                panic!("Should never happens")
            }
        };
    }
    info!("Successful state bootstrap");
    Ok(())
}

// TODO: Change error messages
async fn send_message_client(
    message_to_send: &BootstrapMessageClient,
    client: &mut BootstrapClientBinder,
    write_timeout: Duration,
    read_timeout: Duration,
) -> Result<BootstrapMessageServer, BootstrapError> {
    match tokio::time::timeout(write_timeout, client.send(message_to_send)).await {
        Err(_) => Err(std::io::Error::new(
            std::io::ErrorKind::TimedOut,
            "bootstrap ask ledger part send timed out",
        )
        .into()),
        Ok(Err(e)) => Err(e),
        Ok(Ok(_)) => Ok(()),
    }?;
    match tokio::time::timeout(read_timeout, client.next()).await {
        Err(_) => Err(std::io::Error::new(
            std::io::ErrorKind::TimedOut,
            "final state bootstrap read timed out",
        )
        .into()),
        Ok(Err(e)) => Err(e),
        Ok(Ok(msg)) => Ok(msg),
    }
}

async fn connect_to_server(
    establisher: &mut Establisher,
    bootstrap_settings: &BootstrapSettings,
    addr: &SocketAddr,
    pub_key: &PublicKey,
) -> Result<BootstrapClientBinder, BootstrapError> {
    // connect
    let mut connector = establisher
        .get_connector(bootstrap_settings.connect_timeout)
        .await?; // cancellable
    let socket = connector.connect(*addr).await?; // cancellable
    Ok(BootstrapClientBinder::new(socket, *pub_key))
}

/// Gets the state from a bootstrap server
/// needs to be CANCELLABLE
pub async fn get_state(
    bootstrap_settings: &'static BootstrapSettings,
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
        return Ok(GlobalBootstrapState::new(final_state.clone()));
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
    // Will be none when bootstrap is over
    let mut next_message_bootstrap: Option<BootstrapMessageClient> =
        Some(BootstrapMessageClient::AskFinalStatePart {
            cursor: None,
            slot: None,
            last_async_message_id: None,
        });
    let mut global_bootstrap_state = GlobalBootstrapState::new(final_state.clone());
    loop {
        for (addr, pub_key) in shuffled_list.iter() {
            if let Some(end) = end_timestamp {
                if MassaTime::now().expect("could not get now time") > end {
                    panic!("This episode has come to an end, please get the latest testnet node version to continue");
                }
            }
            info!("Start bootstrapping from {}", addr);
            match connect_to_server(&mut establisher, bootstrap_settings, addr, pub_key).await {
                Ok(mut client) => {
                    match bootstrap_from_server(bootstrap_settings, &mut client, &mut next_message_bootstrap, &mut global_bootstrap_state,version)
                    .await  // cancellable
                    {
                        Err(BootstrapError::ReceivedError(error)) => warn!("Error received from bootstrap server: {}", error),
                        Err(e) => {
                            warn!("Error while bootstrapping: {}", e);
                            // We allow unused result because we don't care if an error is thrown when sending the error message to the server we will close the socket anyway.
                            let _ = tokio::time::timeout(bootstrap_settings.write_error_timeout.into(), client.send(&BootstrapMessageClient::BootstrapError { error: e.to_string() })).await;
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

            info!("Bootstrap from server {} failed. Your node will try to bootstrap from another server in {:#?}.", addr, bootstrap_settings.retry_delay.to_duration());
            sleep(bootstrap_settings.retry_delay.into()).await;
        }
    }
}
