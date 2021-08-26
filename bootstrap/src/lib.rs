// Copyright (c) 2021 MASSA LABS <info@massa.net>

use binders::{ReadBinder, WriteBinder};
use communication::network::{BootstrapPeers, NetworkCommandSender};
use consensus::{BootstrapableGraph, ConsensusCommandSender, ExportProofOfStake};
use establisher::{ReadHalf, WriteHalf};
use log::{debug, info, warn};
use std::net::SocketAddr;

use logging::massa_trace;
mod binders;
pub mod config;
mod error;
pub mod establisher;
mod messages;

use config::BootstrapConfig;
use crypto::{
    hash::Hash,
    signature::{sign, verify_signature, PrivateKey, PublicKey},
};
use error::BootstrapError;
pub use establisher::Establisher;
use messages::BootstrapMessage;
use models::{SerializeCompact, Version};
use rand::{prelude::SliceRandom, rngs::StdRng, RngCore, SeedableRng};
use std::convert::TryInto;
use time::UTime;
use tokio::{sync::mpsc, task::JoinHandle, time::sleep};

async fn get_state_internal(
    cfg: &BootstrapConfig,
    bootstrap_addr: &SocketAddr,
    bootstrap_public_key: &PublicKey,
    establisher: &mut Establisher,
    our_version: Version,
) -> Result<(ExportProofOfStake, BootstrapableGraph, i64, BootstrapPeers), BootstrapError> {
    massa_trace!("bootstrap.lib.get_state_internal", {});
    info!("Start bootstrapping from {}", bootstrap_addr);

    // connect and handshake
    let mut connector = establisher.get_connector(cfg.connect_timeout).await?;
    let (socket_reader, socket_writer) = connector.connect(*bootstrap_addr).await?;
    let mut reader = ReadBinder::new(socket_reader);
    let mut writer = WriteBinder::new(socket_writer);

    let mut random_bytes = [0u8; 32];
    StdRng::from_entropy().fill_bytes(&mut random_bytes);
    let send_time_uncompensated = UTime::now(0)?;
    match tokio::time::timeout(
        cfg.write_timeout.into(),
        writer.send(&messages::BootstrapMessage::BootstrapInitiation {
            random_bytes,
            version: our_version,
        }),
    )
    .await
    {
        Err(_) => {
            return Err(std::io::Error::new(
                std::io::ErrorKind::TimedOut,
                "bootstrap peer read timed out",
            )
            .into())
        }
        Ok(Err(e)) => return Err(e),
        Ok(Ok(_)) => {}
    }

    // First, clock sync.
    let (server_time, sig) =
        match tokio::time::timeout(cfg.read_timeout.into(), reader.next()).await {
            Err(_) => {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::TimedOut,
                    "bootstrap clock sync read timed out",
                )
                .into())
            }
            Ok(Err(e)) => return Err(e),
            Ok(Ok(None)) => return Err(BootstrapError::UnexpectedConnectionDrop),
            Ok(Ok(Some((
                _,
                BootstrapMessage::BootstrapTime {
                    server_time,
                    signature,
                    version,
                },
            )))) => {
                if !our_version.is_compatible(&version) {
                    return Err(BootstrapError::IncompatibleVersionError(format!(
                        "remote is running incompatible version: {} (local node version: {})",
                        version, our_version
                    )));
                }
                (server_time, signature)
            }
            Ok(Ok(Some((_, msg)))) => return Err(BootstrapError::UnexpectedMessage(msg)),
        };

    let recv_time_uncompensated = UTime::now(0)?;

    // Check signature.
    let mut signed_data = random_bytes.to_vec();
    signed_data.extend(server_time.to_bytes_compact()?);
    let signed_data_hash = Hash::hash(&signed_data);
    verify_signature(&signed_data_hash, &sig, &bootstrap_public_key)?;

    let ping = recv_time_uncompensated.saturating_sub(send_time_uncompensated);
    if ping > cfg.max_ping {
        return Err(BootstrapError::GeneralError(
            "bootstrap ping too high".into(),
        ));
    }

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
        debug!(
            "Server clock compensation set to: {:?}",
            compensation_millis
        );
        compensation_millis
    } else {
        0
    };

    let sig_prev = sig;

    // Second, get peers
    let (peers, sig) = match tokio::time::timeout(cfg.read_timeout.into(), reader.next()).await {
        Err(_) => {
            return Err(std::io::Error::new(
                std::io::ErrorKind::TimedOut,
                "bootstrap peer read timed out",
            )
            .into())
        }
        Ok(Err(e)) => return Err(e),
        Ok(Ok(None)) => return Err(BootstrapError::UnexpectedConnectionDrop),
        Ok(Ok(Some((_, BootstrapMessage::BootstrapPeers { peers, signature })))) => {
            (peers, signature)
        }
        Ok(Ok(Some((_, msg)))) => return Err(BootstrapError::UnexpectedMessage(msg)),
    };
    // Check signature.
    let mut signed_data = sig_prev.to_bytes().to_vec();
    signed_data.extend(peers.to_bytes_compact()?);
    let signed_data_hash = Hash::hash(&signed_data);
    verify_signature(&signed_data_hash, &sig, &bootstrap_public_key)?;
    let sig_prev = sig;

    // Third, handle state message.
    let (pos, graph, sig) = match tokio::time::timeout(cfg.read_timeout.into(), reader.next()).await
    {
        Err(_) => {
            return Err(std::io::Error::new(
                std::io::ErrorKind::TimedOut,
                "bootstrap state read timed out",
            )
            .into())
        }
        Ok(Err(e)) => return Err(e),
        Ok(Ok(None)) => return Err(BootstrapError::UnexpectedConnectionDrop),
        Ok(Ok(Some((
            _,
            BootstrapMessage::ConsensusState {
                pos,
                graph,
                signature,
            },
        )))) => (pos, graph, signature),
        Ok(Ok(Some((_, msg)))) => return Err(BootstrapError::UnexpectedMessage(msg)),
    };
    // check signature
    let mut signed_data = sig_prev.to_bytes().to_vec();
    signed_data.extend(pos.to_bytes_compact()?);
    signed_data.extend(graph.to_bytes_compact()?);
    let signed_data_hash = Hash::hash(&signed_data);
    verify_signature(&signed_data_hash, &sig, &bootstrap_public_key)?;
    info!("Successful bootstrap");

    Ok((pos, graph, compensation_millis, peers))
}

pub async fn get_state(
    cfg: BootstrapConfig,
    mut establisher: Establisher,
    version: Version,
    genesis_timestamp: UTime,
    end_timestamp: Option<UTime>,
) -> Result<
    (
        Option<ExportProofOfStake>,
        Option<BootstrapableGraph>,
        i64,
        Option<BootstrapPeers>,
    ),
    BootstrapError,
> {
    massa_trace!("bootstrap.lib.get_state", {});
    let now = UTime::now(0)?;
    // if we are before genesis, do not bootstrap
    if now < genesis_timestamp {
        massa_trace!("bootstrap.lib.get_state.init_from_scratch", {});
        return Ok((None, None, 0, None));
    }
    // we are after genesis => bootstrap
    massa_trace!("bootstrap.lib.get_state.init_from_others", {});
    if cfg.bootstrap_list.is_empty() {
        return Err(BootstrapError::GeneralError(
            "no bootstrap nodes found in list".into(),
        ));
    }
    let mut shuffled_list = cfg.bootstrap_list.clone();
    shuffled_list.shuffle(&mut StdRng::from_entropy());
    loop {
        for (addr, pub_key) in shuffled_list.iter() {
            if let Some(end) = end_timestamp {
                if UTime::now(0).expect("could not get now time") > end {
                    panic!("This episode has already ended, you can download the next one https://gitlab.com/massalabs/massa");
                }
            }

            match get_state_internal(&cfg, addr, pub_key, &mut establisher, version).await {
                Err(e) => {
                    warn!("error while bootstrapping: {:?}", e);
                    sleep(cfg.retry_delay.into()).await;
                }
                Ok((pos, graph, compensation, peers)) => {
                    return Ok((Some(pos), Some(graph), compensation, Some(peers)))
                }
            }
        }
    }
}

pub struct BootstrapManager {
    join_handle: JoinHandle<Result<(), BootstrapError>>,
    manager_tx: mpsc::Sender<()>,
}

impl BootstrapManager {
    pub async fn stop(self) -> Result<(), BootstrapError> {
        massa_trace!("bootstrap.lib.stop", {});
        if self.manager_tx.send(()).await.is_err() {
            warn!("bootstrap server already dropped");
        }
        let _ = self.join_handle.await?;
        Ok(())
    }
}

pub async fn start_bootstrap_server(
    consensus_command_sender: ConsensusCommandSender,
    network_command_sender: NetworkCommandSender,
    cfg: BootstrapConfig,
    establisher: Establisher,
    private_key: PrivateKey,
    compensation_millis: i64,
    version: Version,
) -> Result<Option<BootstrapManager>, BootstrapError> {
    massa_trace!("bootstrap.lib.start_bootstrap_server", {});
    if let Some(bind) = cfg.bind {
        let (manager_tx, manager_rx) = mpsc::channel::<()>(1);
        let join_handle = tokio::spawn(async move {
            BootstrapServer {
                consensus_command_sender,
                network_command_sender,
                establisher,
                manager_rx,
                bind,
                private_key,
                read_timeout: cfg.read_timeout,
                write_timeout: cfg.write_timeout,
                compensation_millis,
                version,
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
    establisher: Establisher,
    manager_rx: mpsc::Receiver<()>,
    bind: SocketAddr,
    private_key: PrivateKey,
    read_timeout: UTime,
    write_timeout: UTime,
    compensation_millis: i64,
    version: Version,
}

impl BootstrapServer {
    pub async fn run(mut self) -> Result<(), BootstrapError> {
        debug!("starting bootstrap server");
        massa_trace!("bootstrap.lib.run", {});
        let mut listener = self.establisher.get_listener(self.bind).await?;
        loop {
            massa_trace!("bootstrap.lib.run.select", {});
            tokio::select! {
                res = listener.accept() => {
                    massa_trace!("bootstrap.lib.run.select.accept", {});
                    match res {
                        Ok(res)=> {
                            if let Err(e) = self.manage_bootstrap(res).await {
                                debug!("error while managing bootstrap connection: {:?} - bootstrap attempt ignored", e.to_string());
                            }
                        },
                        Err(e) => {
                            debug!("error while accepting bootstrap connection: {:?} - connection attempt ignored", e.to_string());
                        }
                    }
                },
                _ = self.manager_rx.recv() => {
                    massa_trace!("bootstrap.lib.run.select.manager", {});
                    break
                },
            }
        }
        Ok(())
    }

    async fn manage_bootstrap(
        &self,
        (reader, writer, _remote_addr): (ReadHalf, WriteHalf, SocketAddr),
    ) -> Result<(), BootstrapError> {
        massa_trace!("bootstrap.lib.manage_bootstrap", {});
        let mut reader = ReadBinder::new(reader);
        let mut writer = WriteBinder::new(writer);

        // Initiation
        let (random_bytes, other_version) =
            match tokio::time::timeout(self.read_timeout.into(), reader.next()).await {
                Err(_) => {
                    return Err(std::io::Error::new(
                        std::io::ErrorKind::TimedOut,
                        "bootstrap init read timed out",
                    )
                    .into())
                }
                Ok(Err(e)) => return Err(e),
                Ok(Ok(None)) => return Err(BootstrapError::UnexpectedConnectionDrop),
                Ok(Ok(Some((
                    _,
                    BootstrapMessage::BootstrapInitiation {
                        random_bytes,
                        version,
                    },
                )))) => (random_bytes, version),
                Ok(Ok(Some((_, msg)))) => return Err(BootstrapError::UnexpectedMessage(msg)),
            };

        // check version
        if !self.version.is_compatible(&other_version) {
            return Err(BootstrapError::IncompatibleVersionError(format!(
                "remote is running incompatible version: {} (local node version: {})",
                other_version, self.version
            )));
        }

        // First, sync clocks.
        let server_time = UTime::now(self.compensation_millis)?;
        let mut signed_data = random_bytes.to_vec();
        signed_data.extend(server_time.to_bytes_compact()?);
        let signed_data_hash = Hash::hash(&signed_data);
        let signature = sign(&signed_data_hash, &self.private_key)?;
        match tokio::time::timeout(
            self.write_timeout.into(),
            writer.send(&messages::BootstrapMessage::BootstrapTime {
                server_time,
                signature,
                version: self.version,
            }),
        )
        .await
        {
            Err(_) => {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::TimedOut,
                    "bootstrap clock send timed out",
                )
                .into())
            }
            Ok(Err(e)) => return Err(e),
            Ok(Ok(_)) => {}
        }
        let last_sig = signature;

        // Second, send peers
        let peers = self.network_command_sender.get_bootstrap_peers().await?;
        let mut signed_data = vec![];
        signed_data.extend(&last_sig.into_bytes());
        signed_data.extend(peers.to_bytes_compact()?);
        let signed_data_hash = Hash::hash(&signed_data);
        let signature = sign(&signed_data_hash, &self.private_key)?;
        match tokio::time::timeout(
            self.write_timeout.into(),
            writer.send(&messages::BootstrapMessage::BootstrapPeers { peers, signature }),
        )
        .await
        {
            Err(_) => {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::TimedOut,
                    "bootstrap clock send timed out",
                )
                .into())
            }
            Ok(Err(e)) => return Err(e),
            Ok(Ok(_)) => {}
        }
        let last_sig = signature;

        // Third, send consensus state.
        let (pos, graph) = self.consensus_command_sender.get_bootstrap_state().await?;
        let mut signed_data = vec![];
        signed_data.extend(&last_sig.into_bytes());
        signed_data.extend(pos.to_bytes_compact()?);
        signed_data.extend(graph.to_bytes_compact()?);
        let signed_data_hash = Hash::hash(&signed_data);
        let signature = sign(&signed_data_hash, &self.private_key)?;
        match tokio::time::timeout(
            self.write_timeout.into(),
            writer.send(&messages::BootstrapMessage::ConsensusState {
                pos,
                graph,
                signature,
            }),
        )
        .await
        {
            Err(_) => {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::TimedOut,
                    "bootstrap graph send timed out",
                )
                .into())
            }
            Ok(Err(e)) => return Err(e),
            Ok(Ok(_)) => {}
        }

        Ok(())
    }
}

#[cfg(test)]
pub mod tests;
