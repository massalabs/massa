use binders::{ReadBinder, WriteBinder};
use consensus::{BoostrapableGraph, ConsensusCommandSender};
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
    signature::{PrivateKey, SignatureEngine},
};
use error::BootstrapError;
pub use establisher::Establisher;
use messages::BootstrapMessage;
use models::{SerializationContext, SerializeCompact};
use rand::{rngs::StdRng, RngCore, SeedableRng};
use std::convert::TryInto;
use time::UTime;
use tokio::{sync::mpsc, task::JoinHandle, time::sleep};

async fn get_state_internal(
    cfg: &BootstrapConfig,
    bootstrap_addr: SocketAddr,
    serialization_context: SerializationContext,
    establisher: &mut Establisher,
) -> Result<(BoostrapableGraph, i64), BootstrapError> {
    massa_trace!("bootstrap.lib.get_state_internal", {});
    let mut random_bytes = [0u8; 32];
    StdRng::from_entropy().fill_bytes(&mut random_bytes);
    // create listener
    let mut connector = establisher.get_connector(cfg.connect_timeout).await?;
    let (socket_reader, socket_writer) = connector.connect(bootstrap_addr).await?;
    let mut reader = ReadBinder::new(socket_reader, serialization_context.clone());
    let mut writer = WriteBinder::new(socket_writer, serialization_context.clone());
    let signature_engine = SignatureEngine::new();

    let send_time_uncompensated = UTime::now(0)?;
    writer
        .send(&messages::BootstrapMessage::BootstrapInitiation { random_bytes })
        .await?;

    // First, clock sync.
    let msg = reader.next().await?.map_or_else(
        || Err(BootstrapError::UnexpectedConnectionDrop),
        |(_, msg)| Ok(msg),
    )?;
    let (server_time, first_sig) = match msg {
        BootstrapMessage::BootstrapTime {
            server_time,
            signature,
        } => (server_time, signature),
        e => return Err(BootstrapError::UnexpectedMessage(e)),
    };

    let recv_time_uncompensated = UTime::now(0)?;

    // Check signature.
    let mut signed_data = random_bytes.to_vec();
    signed_data.extend(server_time.to_bytes_compact(&serialization_context)?);
    let random_and_server_time = Hash::hash(&signed_data);
    signature_engine.verify(
        &random_and_server_time,
        &first_sig,
        &cfg.bootstrap_public_key,
    )?;

    let ping = recv_time_uncompensated.saturating_sub(send_time_uncompensated);
    if ping > cfg.max_ping {
        return Err(BootstrapError::GeneralBootstrapError(
            "bootstrap ping too high".into(),
        ));
    }

    let local_time_uncompensated = recv_time_uncompensated.checked_sub(ping.checked_div_u64(2)?)?;
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
        BootstrapError::GeneralBootstrapError("Failed to convert compensation time into i64".into())
    })?;
    debug!(
        "Server clock compensation set to: {:?}",
        compensation_millis
    );

    // Second, handle state message.
    let msg = reader.next().await?.map_or_else(
        || Err(BootstrapError::UnexpectedConnectionDrop),
        |(_, msg)| Ok(msg),
    )?;
    let (graph, sig) = match msg {
        BootstrapMessage::ConsensusState { graph, signature } => (graph, signature),
        e => return Err(BootstrapError::UnexpectedMessage(e)),
    };

    let mut signed_data = vec![];
    signed_data.extend(&first_sig.into_bytes());
    signed_data.extend(graph.to_bytes_compact(&serialization_context)?);

    let previous_signature_and_message_hash = Hash::hash(&signed_data);
    // check their signature
    signature_engine.verify(
        &previous_signature_and_message_hash,
        &sig,
        &cfg.bootstrap_public_key,
    )?;
    info!("Bootstrap completed successfully 🦀");
    Ok((graph, compensation_millis))
}

pub async fn get_state(
    mut cfg: BootstrapConfig,
    serialization_context: SerializationContext,
    mut establisher: Establisher,
) -> Result<(Option<BoostrapableGraph>, i64), BootstrapError> {
    massa_trace!("bootstrap.lib.get_state", {});
    if let Some(addr) = cfg.bootstrap_addr.take() {
        loop {
            match get_state_internal(&cfg, addr, serialization_context.clone(), &mut establisher)
                .await
            {
                Err(e) => {
                    warn!("error {:?} while bootstraping", e);
                    sleep(cfg.retry_delay.into()).await;
                }
                Ok((graph, compensation)) => return Ok((Some(graph), compensation)),
            }
        }
    }
    Ok((None, 0))
}

pub struct BootstrapManager {
    join_handle: JoinHandle<Result<(), BootstrapError>>,
    manager_tx: mpsc::Sender<()>,
}

impl BootstrapManager {
    pub async fn stop(self) -> Result<(), BootstrapError> {
        massa_trace!("bootstrap.lib.stop", {});
        if let Err(_) = self.manager_tx.send(()).await {
            warn!("bootstrap server  already dropped");
        }
        let _ = self.join_handle.await?;
        Ok(())
    }
}

pub async fn start_bootstrap_server(
    consensus_command_sender: ConsensusCommandSender,
    cfg: BootstrapConfig,
    serialization_context: SerializationContext,
    establisher: Establisher,
    private_key: PrivateKey,
) -> Result<Option<BootstrapManager>, BootstrapError> {
    massa_trace!("bootstrap.lib.start_bootstrap_server", {});
    if let Some(port) = cfg.bind {
        let (manager_tx, manager_rx) = mpsc::channel::<()>(1);
        let join_handle = tokio::spawn(async move {
            BootstrapServer {
                consensus_command_sender,
                serialization_context,
                establisher,
                manager_rx,
                port,
                private_key,
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
    serialization_context: SerializationContext,
    establisher: Establisher,
    manager_rx: mpsc::Receiver<()>,
    port: SocketAddr,
    private_key: PrivateKey,
}

impl BootstrapServer {
    pub async fn run(mut self) -> Result<(), BootstrapError> {
        debug!("starting bootstrap server");
        massa_trace!("bootstrap.lib.run", {});
        let mut listener = self.establisher.get_listener(self.port).await?;
        loop {
            massa_trace!("bootstrap.lib.run.select", {});
            tokio::select! {
                res = listener.accept() => {

            massa_trace!("bootstrap.lib.run.select.accept", {});
                    match res {
                        Ok(res)=> {
                            if let Err(e) = self.manage_bootstrap(res).await {
                                warn!("error while managing bootstrap connection: {:?} - bootstrap attempt ignored", e.to_string());
                            }
                        },
                        Err(e) => {
                            warn!("error while accepting bootstrap connection: {:?} - connection attempt ignored", e.to_string());
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
        let mut reader = ReadBinder::new(reader, self.serialization_context.clone());
        let mut writer = WriteBinder::new(writer, self.serialization_context.clone());

        let msg = reader.next().await?.map_or_else(
            || Err(BootstrapError::UnexpectedConnectionDrop),
            |(_, msg)| Ok(msg),
        )?;
        match msg {
            BootstrapMessage::BootstrapInitiation { random_bytes } => {
                massa_trace!("bootstrap.lib.manage_bootstrap.initiation", {});
                let signature_engine = SignatureEngine::new();

                // First, sync clocks.
                let server_time = UTime::now(0)?;
                let mut signed_data = random_bytes.to_vec();
                signed_data.extend(server_time.to_bytes_compact(&self.serialization_context)?);
                let random_and_server_time = Hash::hash(&signed_data);
                let signature =
                    signature_engine.sign(&random_and_server_time, &self.private_key)?;
                let message = BootstrapMessage::BootstrapTime {
                    server_time,
                    signature: signature.clone(),
                };

                writer.send(&message).await?;

                // Second, send state.
                let graph = self.consensus_command_sender.get_bootstrap_graph().await?;
                let mut signed_data = vec![];
                signed_data.extend(&signature.into_bytes());
                signed_data.extend(graph.to_bytes_compact(&self.serialization_context)?);

                let previous_signature_and_message_hash = Hash::hash(&signed_data);

                let signature = signature_engine
                    .sign(&previous_signature_and_message_hash, &self.private_key)?;
                let message = BootstrapMessage::ConsensusState { graph, signature };

                writer.send(&message).await?;

                Ok(())
            }
            msg => {
                warn!("Unexpected bootstrap message");
                Err(BootstrapError::UnexpectedMessage(msg))
            }
        }
    }
}

#[cfg(test)]
pub mod tests;
