use massa_consensus_exports::{
    commands::{ConsensusCommand, ConsensusManagementCommand},
    error::{ConsensusError, ConsensusResult as Result},
    events::ConsensusEvent,
    settings::{ConsensusChannels, ConsensusWorkerChannels},
    ConsensusCommandSender, ConsensusEventReceiver, ConsensusManager,
};
use tracing::{debug, error, info};

use crate::consensus_worker::ConsensusWorker;
use massa_consensus_exports::settings::ConsensusConfig;
use massa_graph::{settings::GraphConfig, BlockGraph, BootstrapableGraph};
use massa_models::{constants::CHANNEL_SIZE, prehash::Map, Address};
use massa_proof_of_stake_exports::{ExportProofOfStake, ProofOfStake, ProofOfStakeConfig};
use massa_signature::{derive_public_key, PrivateKey, PublicKey};
use std::path::Path;
use tokio::sync::mpsc;

async fn load_initial_staking_keys(path: &Path) -> Result<Map<Address, (PublicKey, PrivateKey)>> {
    if !std::path::Path::is_file(path) {
        return Ok(Map::default());
    }
    serde_json::from_str::<Vec<PrivateKey>>(&tokio::fs::read_to_string(path).await?)?
        .iter()
        .map(|private_key| {
            let public_key = derive_public_key(private_key);
            Ok((
                Address::from_public_key(&public_key),
                (public_key, *private_key),
            ))
        })
        .collect()
}

/// Creates a new consensus controller.
///
/// # Arguments
/// * cfg: consensus configuration
/// * protocol_command_sender: a ProtocolCommandSender instance to send commands to Protocol.
/// * protocol_event_receiver: a ProtocolEventReceiver instance to receive events from Protocol.
pub async fn start_consensus_controller(
    cfg: ConsensusConfig,
    channels: ConsensusChannels,
    boot_pos: Option<ExportProofOfStake>,
    boot_graph: Option<BootstrapableGraph>,
    clock_compensation: i64,
) -> Result<(
    ConsensusCommandSender,
    ConsensusEventReceiver,
    ConsensusManager,
)> {
    debug!("starting consensus controller");
    massa_trace!(
        "consensus.consensus_controller.start_consensus_controller",
        {}
    );

    // todo that is checked when loading the config, should be removed
    // ensure that the parameters are sane
    if cfg.thread_count == 0 {
        return Err(ConsensusError::ConfigError(
            "thread_count shoud be strictly more than 0".to_string(),
        ));
    }
    if cfg.t0 == 0.into() {
        return Err(ConsensusError::ConfigError(
            "t0 shoud be strictly more than 0".to_string(),
        ));
    }
    if cfg.t0.checked_rem_u64(cfg.thread_count as u64)? != 0.into() {
        return Err(ConsensusError::ConfigError(
            "thread_count should divide t0".to_string(),
        ));
    }
    let staking_keys = load_initial_staking_keys(&cfg.staking_keys_path).await?;

    // start worker
    let block_db = BlockGraph::new(GraphConfig::from(&cfg), boot_graph).await?;
    let mut pos = ProofOfStake::new(
        ProofOfStakeConfig::from(&cfg),
        block_db.get_genesis_block_ids(),
        boot_pos,
    )
    .await?;
    pos.set_watched_addresses(staking_keys.keys().copied().collect());
    let (command_tx, command_rx) = mpsc::channel::<ConsensusCommand>(CHANNEL_SIZE);
    let (event_tx, event_rx) = mpsc::channel::<ConsensusEvent>(CHANNEL_SIZE);
    let (manager_tx, manager_rx) = mpsc::channel::<ConsensusManagementCommand>(1);
    let cfg_copy = cfg.clone();
    let join_handle = tokio::spawn(async move {
        let res = ConsensusWorker::new(
            cfg_copy,
            ConsensusWorkerChannels {
                protocol_command_sender: channels.protocol_command_sender,
                protocol_event_receiver: channels.protocol_event_receiver,
                execution_controller: channels.execution_controller,
                pool_command_sender: channels.pool_command_sender,
                controller_command_rx: command_rx,
                controller_event_tx: event_tx,
                controller_manager_rx: manager_rx,
            },
            block_db,
            pos,
            clock_compensation,
            staking_keys,
        )
        .await?
        .run_loop()
        .await;
        match res {
            Err(err) => {
                error!("consensus worker crashed: {}", err);
                Err(err)
            }
            Ok(v) => {
                info!("consensus worker finished cleanly");
                Ok(v)
            }
        }
    });
    Ok((
        ConsensusCommandSender(command_tx),
        ConsensusEventReceiver(event_rx),
        ConsensusManager {
            manager_tx,
            join_handle,
        },
    ))
}
