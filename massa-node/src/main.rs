// Copyright (c) 2021 MASSA LABS <info@massa.net>

#![feature(ip)]
#![feature(destructuring_assignment)]

extern crate logging;
pub use api::ApiEvent;
use api::{start_api_controller, ApiEventReceiver, ApiManager};
// TODO: use api_eth::{EthRpc, API as APIEth};
use api_private::ApiMassaPrivate;
use api_public::ApiMassaPublic;
use bootstrap::{get_state, start_bootstrap_server, BootstrapManager};
use communication::{
    network::{start_network_controller, Establisher, NetworkCommandSender, NetworkManager},
    protocol::{start_protocol_controller, ProtocolManager},
};
use consensus::{
    start_consensus_controller, ConsensusCommandSender, ConsensusEvent, ConsensusEventReceiver,
    ConsensusManager,
};
use human_panic::setup_panic;
use log::{error, info, trace};
use logging::{massa_trace, warn};
use models::OperationHashMap;
use models::{
    init_serialization_context, Address, BlockHashMap, OperationSearchResult,
    OperationSearchResultStatus, SerializationContext,
};
use pool::{start_pool_controller, PoolCommandSender, PoolManager};
use storage::{start_storage, StorageManager};
use time::UTime;
use tokio::signal;
use tokio::sync::mpsc;

mod node_config;

async fn launch(
    cfg: node_config::Config,
) -> (
    PoolCommandSender,
    ConsensusEventReceiver,
    ApiEventReceiver,
    ConsensusCommandSender,
    NetworkCommandSender,
    Option<BootstrapManager>,
    ApiManager,
    ConsensusManager,
    PoolManager,
    ProtocolManager,
    StorageManager,
    NetworkManager,
    mpsc::Receiver<()>,
) {
    info!("Node version : {}", cfg.version);
    if let Some(end) = cfg.consensus.end_timestamp {
        if UTime::now(0).expect("could not get now time") > end {
            panic!("This episode has come to an end, please get the latest testnet node version to continue");
        }
    }

    // Init the global serialization context
    init_serialization_context(SerializationContext {
        max_block_operations: cfg.consensus.max_operations_per_block,
        parent_count: cfg.consensus.thread_count,
        max_block_size: cfg.consensus.max_block_size,
        max_peer_list_length: cfg.network.max_advertise_length,
        max_message_size: cfg.network.max_message_size,
        max_bootstrap_blocks: cfg.bootstrap.max_bootstrap_blocks,
        max_bootstrap_cliques: cfg.bootstrap.max_bootstrap_cliques,
        max_bootstrap_deps: cfg.bootstrap.max_bootstrap_deps,
        max_bootstrap_children: cfg.bootstrap.max_bootstrap_children,
        max_ask_blocks_per_message: cfg.network.max_ask_blocks_per_message,
        max_operations_per_message: cfg.network.max_operations_per_message,
        max_endorsements_per_message: cfg.network.max_endorsements_per_message,
        max_bootstrap_message_size: cfg.bootstrap.max_bootstrap_message_size,
        max_bootstrap_pos_cycles: cfg.bootstrap.max_bootstrap_pos_cycles,
        max_bootstrap_pos_entries: cfg.bootstrap.max_bootstrap_pos_entries,
        max_block_endorsments: cfg.consensus.endorsement_count,
    });

    let (boot_pos, boot_graph, clock_compensation, initial_peers) = get_state(
        cfg.bootstrap.clone(),
        bootstrap::establisher::Establisher::new(),
        cfg.version,
        cfg.consensus.genesis_timestamp,
        cfg.consensus.end_timestamp,
    )
    .await
    .unwrap();

    // launch network controller
    let (network_command_sender, network_event_receiver, network_manager, private_key) =
        start_network_controller(
            cfg.network.clone(),
            Establisher::new(),
            clock_compensation,
            initial_peers,
            cfg.version,
        )
        .await
        .expect("could not start network controller");

    // start storage
    let (storage_command_sender, storage_manager) =
        start_storage(cfg.storage.clone()).expect("could not start storage controller");

    // launch protocol controller
    let (
        protocol_command_sender,
        protocol_event_receiver,
        protocol_pool_event_receiver,
        protocol_manager,
    ) = start_protocol_controller(
        cfg.protocol.clone(),
        cfg.consensus.operation_validity_periods,
        network_command_sender.clone(),
        network_event_receiver,
    )
    .await
    .expect("could not start protocol controller");

    // launch pool controller
    let (pool_command_sender, pool_manager) = start_pool_controller(
        cfg.pool.clone(),
        cfg.consensus.thread_count,
        cfg.consensus.operation_validity_periods,
        protocol_command_sender.clone(),
        protocol_pool_event_receiver,
    )
    .await
    .expect("could not start pool controller");

    // launch consensus controller
    let (consensus_command_sender, consensus_event_receiver, consensus_manager) =
        start_consensus_controller(
            cfg.consensus.clone(),
            protocol_command_sender.clone(),
            protocol_event_receiver,
            pool_command_sender.clone(),
            Some(storage_command_sender.clone()),
            boot_pos,
            boot_graph,
            clock_compensation,
        )
        .await
        .expect("could not start consensus controller");

    // launch API controller
    let (api_event_receiver, api_manager) = start_api_controller(
        cfg.version,
        cfg.api.clone(),
        cfg.consensus.clone(),
        cfg.protocol.clone(),
        cfg.network.clone(),
        cfg.pool.clone(),
        Some(storage_command_sender.clone()),
        clock_compensation,
    )
    .await
    .expect("could not start API controller");

    let bootstrap_manager = start_bootstrap_server(
        consensus_command_sender.clone(),
        network_command_sender.clone(),
        cfg.bootstrap,
        bootstrap::Establisher::new(),
        private_key,
        clock_compensation,
        cfg.version,
    )
    .await
    .unwrap();

    // spawn APIs
    let (api_private, api_private_stop_rx) = ApiMassaPrivate::create(
        &cfg.new_api.bind_private.to_string(),
        consensus_command_sender.clone(),
        network_command_sender.clone(),
        cfg.new_api.clone(),
        cfg.consensus.clone(),
    );
    api_private.serve_massa_private();

    let api_public = ApiMassaPublic::create(
        &cfg.new_api.bind_public.to_string(),
        consensus_command_sender.clone(),
        cfg.new_api,
        cfg.consensus,
        pool_command_sender.clone(),
        Some(storage_command_sender),
        cfg.network,
        cfg.version,
        network_command_sender.clone(),
        clock_compensation,
    );
    api_public.serve_massa_public();

    // TODO: This will implemented later ...
    // let api_eth = APIEth::from_url("127.0.0.1:33036");
    // api_eth.serve_eth_rpc();

    (
        pool_command_sender,
        consensus_event_receiver,
        api_event_receiver,
        consensus_command_sender,
        network_command_sender,
        bootstrap_manager,
        api_manager,
        consensus_manager,
        pool_manager,
        protocol_manager,
        storage_manager,
        network_manager,
        api_private_stop_rx,
    )
}

// FIXME: IDEA identify it unreachable code?
async fn run(cfg: node_config::Config) {
    loop {
        let (
            pool_command_sender,
            mut consensus_event_receiver,
            mut api_event_receiver,
            consensus_command_sender,
            network_command_sender,
            bootstrap_manager,
            api_manager,
            consensus_manager,
            pool_manager,
            protocol_manager,
            storage_manager,
            network_manager,
            mut api_private_stop_rx,
        ) = launch(cfg.clone()).await;

        // interrupt signal listener
        let stop_signal = signal::ctrl_c();
        tokio::pin!(stop_signal);
        // loop over messages
        let restart = loop {
            massa_trace!("massa-node.main.run.select", {});
            let mut api_pool_command_sender = pool_command_sender.clone();
            tokio::select! {
                evt = consensus_event_receiver.wait_event() => {
                    massa_trace!("massa-node.main.run.select.consensus_event", {});
                    match evt {
                        Ok(ConsensusEvent::NeedSync) => {
                            warn!("in response to a desynchronization, the node is going to bootstrap again");
                            break true;
                        },
                        Err(err) => {
                            error!("consensus_event_receiver.wait_event error: {:?}", err);
                            break false ;
                        }
                    }
                },

                evt = api_event_receiver.wait_event() =>{
                    massa_trace!("massa-node.main.run.select.api_event", {});

                    if on_api_event(evt.map_err(|e|format!("api communication error: {:?}", e)).unwrap(),
                    &mut api_pool_command_sender,
                    &consensus_command_sender,
                    &network_command_sender).await {break false}
                }

                _ = &mut stop_signal => {
                    massa_trace!("massa-node.main.run.select.stop", {});
                    info!("interrupt signal received");
                    break false;
                }

                _ = api_private_stop_rx.recv() => {
                    info!("stop command received from private API");
                    break false;
                }
            }
        };
        stop(
            bootstrap_manager,
            api_manager,
            api_event_receiver,
            consensus_manager,
            consensus_event_receiver,
            pool_manager,
            protocol_manager,
            storage_manager,
            network_manager,
        )
        .await;
        if !restart {
            break;
        }
    }
}

async fn on_api_event(
    evt: ApiEvent,
    api_pool_command_sender: &mut PoolCommandSender,
    consensus_command_sender: &ConsensusCommandSender,
    network_command_sender: &NetworkCommandSender,
) -> bool {
    match evt {
        ApiEvent::AddOperations(operations) => {
            massa_trace!("massa-node.main.run.select.api_event.AddOperations", {
                "operations": operations
            });
            for (id, op) in operations.iter() {
                let from_address = match Address::from_public_key(&op.content.sender_public_key) {
                    Ok(addr) => addr.to_string(),
                    Err(_) => "could not get address from public key".to_string(),
                };
                let operation_message = match &op.content.op {
                    models::OperationType::Transaction {
                        amount,
                        recipient_address,
                    } => format!(
                        "transaction from address {} to address {}, with amount {}",
                        from_address, recipient_address, amount
                    ),
                    models::OperationType::RollBuy { roll_count } => {
                        format!("address {} buys {} rolls", from_address, roll_count)
                    }
                    models::OperationType::RollSell { roll_count } => {
                        format!("address {} sells {} rolls", from_address, roll_count)
                    }
                };
                info!(
                    "Added operation {} from API: {}, fee {}",
                    id, operation_message, op.content.fee
                );
            }
            if api_pool_command_sender
                .add_operations(operations)
                .await
                .is_err()
            {
                warn!("could not send AddOperations to pool in api_event_receiver.wait_event");
            }
        }
        ApiEvent::AskStop => {
            info!("API asked node stop");
            return true;
        }
        ApiEvent::GetBlockStatus {
            block_id,
            response_tx,
        } => {
            massa_trace!("massa-node.main.run.select.api_event.get_active_block", {
                "block_id": block_id
            });
            if response_tx
                .send(
                    consensus_command_sender
                        .get_block_status(block_id)
                        .await
                        .expect("get_active_block failed in api_event_receiver.wait_event"),
                )
                .is_err()
            {
                warn!("could not send get_active_block response in api_event_receiver.wait_event");
            }
        }
        ApiEvent::GetBlockGraphStatus(response_sender_tx) => {
            massa_trace!(
                "massa-node.main.run.select.api_event.get_block_graph_status",
                {}
            );
            if response_sender_tx
                .send(
                    consensus_command_sender
                        .get_block_graph_status()
                        .await
                        .expect("get_block_graph_status failed in api_event_receiver.wait_event"),
                )
                .is_err()
            {
                warn!("could not send get_block_graph_status response in api_event_receiver.wait_event");
            }
            trace!("after sending block graph to response_tx sender in loop in massa-node main");
        }
        ApiEvent::GetPeers(response_sender_tx) => {
            massa_trace!("massa-node.main.run.select.api_event.get_peers", {});
            if response_sender_tx
                .send(
                    network_command_sender
                        .get_peers()
                        .await
                        .expect("get_peers failed in api_event_receiver.wait_event"),
                )
                .is_err()
            {
                warn!("could not send get_peers response in api_event_receiver.wait_event");
            }
        }
        ApiEvent::GetSelectionDraw {
            start,
            end,
            response_tx,
        } => {
            massa_trace!(
                "massa-node.main.run.select.api_event.get_selection_draws",
                {}
            );
            if response_tx
                .send(
                    consensus_command_sender
                        .get_selection_draws(start, end)
                        .await,
                )
                .is_err()
            {
                warn!(
                    "could not send get_selection_draws response in api_event_receiver.wait_event"
                );
            }
        }
        ApiEvent::GetAddressesInfo {
            addresses,
            response_tx,
        } => {
            massa_trace!("massa-node.main.run.select.api_event.get_addresses_info", {
            });
            if response_tx
                .send(consensus_command_sender.get_addresses_info(addresses).await)
                .is_err()
            {
                warn!(
                    "could not send get_addresses_info response in api_event_receiver.wait_event"
                );
            }
        }
        ApiEvent::GetRecentOperations {
            address,
            response_tx,
        } => {
            massa_trace!(
                "massa-node.main.run.select.api_event.get_operations_involving_address",
                {}
            );
            let mut res: OperationHashMap<_> = api_pool_command_sender
                .get_operations_involving_address(address)
                .await
                .expect("could not get recent operations from pool");

            consensus_command_sender
                .get_operations_involving_address(address)
                .await
                .expect("could not retrieve recent operations from consensus")
                .into_iter()
                .for_each(|(op_id, search_new)| {
                    res.entry(op_id)
                        .and_modify(|search_old| search_old.extend(&search_new))
                        .or_insert(search_new);
                });

            if response_tx.send(res).is_err() {
                warn!("could not send get_operations_involving_address response in api_event_receiver.wait_event");
            }
        }
        ApiEvent::GetOperations {
            operation_ids,
            response_tx,
        } => {
            massa_trace!("massa-node.main.run.select.api_event.get_operations", {
                "operation_ids": operation_ids
            });
            let mut res: OperationHashMap<OperationSearchResult> = api_pool_command_sender
                .get_operations(operation_ids.iter().cloned().collect())
                .await
                .expect("could not get operations from pool")
                .into_iter()
                .map(|(id, op)| {
                    (
                        id,
                        OperationSearchResult {
                            in_pool: true,
                            in_blocks: BlockHashMap::default(),
                            op,
                            status: OperationSearchResultStatus::Pending,
                        },
                    )
                })
                .collect();

            consensus_command_sender
                .get_operations(operation_ids.iter().cloned().collect())
                .await
                .expect("could not get opeatrions frim consensus")
                .into_iter()
                .for_each(|(op_id, search_new)| {
                    res.entry(op_id)
                        .and_modify(|search_old| search_old.extend(&search_new))
                        .or_insert(search_new);
                });
            if response_tx.send(res).is_err() {
                warn!("could not send get_operations response in api_event_receiver.wait_event");
            }
        }
        ApiEvent::GetStats(response_tx) => {
            massa_trace!("massa-node.main.run.select.api_event.get_stats", {});
            if response_tx
                .send(
                    consensus_command_sender
                        .get_stats()
                        .await
                        .expect("get_stats failed in api_event_receiver.wait_event"),
                )
                .is_err()
            {
                warn!("could not send get_stats response in api_event_receiver.wait_event");
            }
        }
        ApiEvent::GetActiveStakers(response_tx) => {
            massa_trace!("massa-node.main.run.select.api_event.get_active_stakers", {
            });
            if response_tx
                .send(
                    consensus_command_sender
                        .get_active_stakers()
                        .await
                        .expect("get_active_stakers failed in api_event_receiver.wait_event"),
                )
                .is_err()
            {
                warn!(
                    "could not send get_active_stakers response in api_event_receiver.wait_event"
                );
            }
        }
        ApiEvent::RegisterStakingPrivateKeys(key) => {
            massa_trace!(
                "massa-node.main.run.select.api_event.register_staking_private_keys",
                {}
            );
            consensus_command_sender
                .register_staking_private_keys(key)
                .await
                .expect("register_staking_private_keys failed in api_event_receiver.wait_event")
        }
        ApiEvent::RemoveStakingAddresses(address) => {
            massa_trace!(
                "massa-node.main.run.select.api_event.remove_staking_addresses",
                {}
            );
            consensus_command_sender
                .remove_staking_addresses(address)
                .await
                .expect("remove_staking_addresses failed in api_event_receiver.wait_event")
        }
        ApiEvent::Unban(ip) => {
            massa_trace!("massa-node.main.run.select.api_event.unban", {});
            network_command_sender
                .unban(vec![ip])
                .await
                .expect("unban failed in api_event_receiver.wait_event")
        }
        ApiEvent::GetStakingAddresses(response_tx) => {
            massa_trace!(
                "massa-node.main.run.select.api_event.get_staking_addresses",
                {}
            );
            if response_tx
                .send(
                    consensus_command_sender
                        .get_staking_addresses()
                        .await
                        .expect("get_staking_addresses failed in api_event_receiver.wait_event"),
                )
                .is_err()
            {
                warn!("could not send get_staking_addresses response in api_event_receiver.wait_event");
            }
        }
        ApiEvent::NodeSignMessage {
            message,
            response_tx,
        } => {
            massa_trace!("massa-node.main.run.select.api_event.node_sign_message", {});
            if response_tx
                .send(
                    network_command_sender
                        .node_sign_message(message)
                        .await
                        .expect("node_sign_message failed in api_event_receiver.wait_event"),
                )
                .is_err()
            {
                warn!("could not send node_sign_message response in api_event_receiver.wait_event");
            }
        }
        ApiEvent::GetStakersProductionStats { addrs, response_tx } => {
            massa_trace!(
                "massa-node.main.run.select.api_event.get_stakers_production_stats",
                {}
            );
            if response_tx
                .send(
                    consensus_command_sender
                        .get_stakers_production_stats(addrs)
                        .await
                        .expect(
                            "get stakers production stats failed in api_event_receiver.wait_event",
                        ),
                )
                .is_err()
            {
                warn!("could not send get_staker_production_stats response in api_event_receiver.wait_event");
            }
        }
        ApiEvent::GetBlockIdsByCreator {
            address,
            response_tx,
        } => {
            massa_trace!(
                "massa-node.main.run.select.api_event.get_block_ids_by_creator",
                {}
            );
            if response_tx
                .send(
                    consensus_command_sender
                        .get_block_ids_by_creator(address)
                        .await
                        .expect("get_block_ids_by_creator failed in api_event_receiver.wait_event"),
                )
                .is_err()
            {
                warn!("could not send get_block_ids_by_creator response in api_event_receiver.wait_event");
            }
        }
    }
    false
}

async fn stop(
    bootstrap_manager: Option<BootstrapManager>,
    api_manager: ApiManager,
    api_event_receiver: ApiEventReceiver,
    consensus_manager: ConsensusManager,
    consensus_event_receiver: ConsensusEventReceiver,
    pool_manager: PoolManager,
    protocol_manager: ProtocolManager,
    storage_manager: StorageManager,
    network_manager: NetworkManager,
) {
    // stop bootstrap
    if let Some(bootstrap_manager) = bootstrap_manager {
        bootstrap_manager
            .stop()
            .await
            .expect("bootstrap server shutdown failed")
    }

    // stop API controller
    let _remaining_api_events = api_manager
        .stop(api_event_receiver)
        .await
        .expect("API shutdown failed");

    // stop consensus controller
    let protocol_event_receiver = consensus_manager
        .stop(consensus_event_receiver)
        .await
        .expect("consensus shutdown failed");

    // stop pool controller
    let protocol_pool_event_receiver = pool_manager.stop().await.expect("pool shutdown failed");

    // stop protocol controller
    let network_event_receiver = protocol_manager
        .stop(protocol_event_receiver, protocol_pool_event_receiver)
        .await
        .expect("protocol shutdown failed");

    //stop storage controller
    storage_manager
        .stop()
        .await
        .expect("storage shutdown failed");

    // stop network controller
    network_manager
        .stop(network_event_receiver)
        .await
        .expect("network shutdown failed");
}

#[tokio::main]
async fn main() {
    setup_panic!();
    // load config
    let config_path = "base_config/config.toml";
    let override_config_path = "config/config.toml";
    let mut cfg = config::Config::default();
    cfg.merge(config::File::with_name(config_path))
        .expect("could not load main config file");
    if std::path::Path::new(override_config_path).is_file() {
        cfg.merge(config::File::with_name(override_config_path))
            .expect("could not load override config file");
    }
    let cfg = cfg
        .try_into::<node_config::Config>()
        .expect("error structuring config");

    // setup logging
    stderrlog::new()
        .module(module_path!())
        .module("bootstrap")
        .module("communication")
        .module("consensus")
        .module("crypto")
        .module("logging")
        .module("models")
        .module("time")
        .module("api")
        .module("pool")
        .verbosity(cfg.logging.level)
        .timestamp(stderrlog::Timestamp::Millisecond)
        .init()
        .unwrap();

    run(cfg).await
}
