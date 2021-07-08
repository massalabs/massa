use binders::{ReadBinder, WriteBinder};
use consensus::{BoostrapableGraph, ConsensusCommandSender};
use establisher::{ReadHalf, WriteHalf};
use log::{debug, warn};
use std::net::SocketAddr;

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
use time::UTime;
use tokio::{sync::mpsc, task::JoinHandle, time::sleep};

async fn get_state_internal(
    cfg: BootstrapConfig,
    serialization_context: SerializationContext,
    establisher: &mut Establisher,
) -> Result<BoostrapableGraph, BootstrapError> {
    let mut random_bytes = [0u8; 32];
    StdRng::from_entropy().fill_bytes(&mut random_bytes);
    // create listener
    let bootstrap_addr = SocketAddr::new(cfg.bootstrap_ip, cfg.bootstrap_port);
    let mut connector = establisher.get_connector(cfg.connect_timeout).await?;
    let (socket_reader, socket_writer) = connector.connect(bootstrap_addr).await?;
    let mut reader = ReadBinder::new(socket_reader, serialization_context.clone());
    let mut writer = WriteBinder::new(socket_writer, serialization_context.clone());

    writer
        .send(&messages::BootstrapMessage::BootstrapInitiation { random_bytes })
        .await?;
    let msg = reader.next().await?.map_or_else(
        || Err(BootstrapError::UnexpectedConnectionDrop),
        |(_, msg)| Ok(msg),
    )?;
    let (graph, sig) = match msg {
        BootstrapMessage::ConsensusState { graph, signature } => (graph, signature),
        e => return Err(BootstrapError::UnexpectedMessage(e)),
    };

    let mut signed_data = random_bytes.to_vec();
    signed_data.extend(graph.to_bytes_compact(&serialization_context)?);

    let random_and_message_hash = Hash::hash(&signed_data);
    let signature_engine = SignatureEngine::new();
    // check their signature
    signature_engine.verify(&random_and_message_hash, &sig, &cfg.bootstrap_public_key)?;
    Ok(graph)
}

pub async fn get_state(
    cfg: BootstrapConfig,
    serialization_context: SerializationContext,
    mut establisher: Establisher,
    genesis_time: UTime,
) -> Result<Option<BoostrapableGraph>, BootstrapError> {
    let now = UTime::now()?;
    if now < genesis_time.saturating_add(cfg.bootstrap_time_after_genesis) {
        return Ok(None);
    } else {
        loop {
            match get_state_internal(cfg.clone(), serialization_context.clone(), &mut establisher)
                .await
            {
                Err(e) => {
                    warn!("error {:?} while bootstraping", e);
                    sleep(cfg.retry_delay.into()).await;
                }
                Ok(graph) => break Ok(Some(graph)),
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
        let mut listener = self.establisher.get_listener(self.port).await?;
        loop {
            tokio::select! {
                res = listener.accept() => match res {
                    Ok(res)=> {
                        if let Err(e) = self.manage_bootstrap(res).await {
                            warn!("error while managing bootstrap connection: {:?} - bootstrap attempt ignored", e.to_string());
                        }
                    },
                    Err(e) => {
                        warn!("error while accepting bootstrap connection: {:?} - connection attempt ignored", e.to_string());
                    }
                },
                _ = self.manager_rx.recv() => break,
            }
        }
        Ok(())
    }

    async fn manage_bootstrap(
        &self,
        (reader, writer, _remote_addr): (ReadHalf, WriteHalf, SocketAddr),
    ) -> Result<(), BootstrapError> {
        let mut reader = ReadBinder::new(reader, self.serialization_context.clone());
        let mut writer = WriteBinder::new(writer, self.serialization_context.clone());

        let msg = reader.next().await?.map_or_else(
            || Err(BootstrapError::UnexpectedConnectionDrop),
            |(_, msg)| Ok(msg),
        )?;
        match msg {
            BootstrapMessage::BootstrapInitiation { random_bytes } => {
                let graph = self.consensus_command_sender.get_bootstrap_graph().await?;
                let mut signed_data = random_bytes.to_vec();
                signed_data.extend(graph.to_bytes_compact(&self.serialization_context)?);

                let random_and_message_hash = Hash::hash(&signed_data);
                let signature_engine = SignatureEngine::new();
                let signature =
                    signature_engine.sign(&random_and_message_hash, &self.private_key)?;
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
