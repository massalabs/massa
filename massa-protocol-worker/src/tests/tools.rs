use crate::start_protocol_controller;
use futures::Future;
use massa_graph_2_exports::test_exports::{
    GraphEventReceiver, MockGraphController, MockGraphControllerMessage,
};
use massa_models::{
    block::{BlockId, WrappedBlock},
    node::NodeId,
    operation::WrappedOperation,
    prehash::PreHashSet,
};
use massa_network_exports::BlockInfoReply;
use massa_pool_exports::test_exports::{MockPoolController, PoolEventReceiver};
use massa_protocol_exports::{
    tests::mock_network_controller::MockNetworkController, ProtocolCommandSender, ProtocolConfig,
    ProtocolManager,
};
use massa_storage::Storage;
use massa_time::MassaTime;
use tokio::sync::mpsc;

pub async fn protocol_test<F, V>(protocol_config: &ProtocolConfig, test: F)
where
    F: FnOnce(
        MockNetworkController,
        ProtocolCommandSender,
        ProtocolManager,
        GraphEventReceiver,
        PoolEventReceiver,
    ) -> V,
    V: Future<
        Output = (
            MockNetworkController,
            ProtocolCommandSender,
            ProtocolManager,
            GraphEventReceiver,
            PoolEventReceiver,
        ),
    >,
{
    let (network_controller, network_command_sender, network_event_receiver) =
        MockNetworkController::new();

    let (pool_controller, pool_event_receiver) = MockPoolController::new_with_receiver();
    let (graph_controller, graph_event_receiver) = MockGraphController::new_with_receiver();
    // start protocol controller
    let (protocol_command_sender, protocol_command_receiver) =
        mpsc::channel(protocol_config.controller_channel_size);
    // start protocol controller
    let protocol_manager: ProtocolManager = start_protocol_controller(
        *protocol_config,
        network_command_sender,
        network_event_receiver,
        protocol_command_receiver,
        graph_controller,
        pool_controller,
        Storage::create_root(),
    )
    .await
    .expect("could not start protocol controller");

    let protocol_command_sender = ProtocolCommandSender(protocol_command_sender);
    let (
        _network_controller,
        _protocol_command_sender,
        protocol_manager,
        _graph_event_receiver,
        _pool_event_receiver,
    ) = test(
        network_controller,
        protocol_command_sender,
        protocol_manager,
        graph_event_receiver,
        pool_event_receiver,
    )
    .await;

    protocol_manager
        .stop()
        .await
        .expect("Failed to shutdown protocol.");
}

pub async fn protocol_test_with_storage<F, V>(protocol_config: &ProtocolConfig, test: F)
where
    F: FnOnce(
        MockNetworkController,
        ProtocolCommandSender,
        ProtocolManager,
        GraphEventReceiver,
        PoolEventReceiver,
        Storage,
    ) -> V,
    V: Future<
        Output = (
            MockNetworkController,
            ProtocolCommandSender,
            ProtocolManager,
            GraphEventReceiver,
            PoolEventReceiver,
        ),
    >,
{
    let (network_controller, network_command_sender, network_event_receiver) =
        MockNetworkController::new();
    let (pool_controller, mock_pool_receiver) = MockPoolController::new_with_receiver();
    let (graph_controller, mock_graph_receiver) = MockGraphController::new_with_receiver();
    let storage = Storage::create_root();
    // start protocol controller
    let (protocol_command_sender, protocol_command_receiver) =
        mpsc::channel(protocol_config.controller_channel_size);
    let protocol_manager = start_protocol_controller(
        *protocol_config,
        network_command_sender,
        network_event_receiver,
        protocol_command_receiver,
        graph_controller,
        pool_controller,
        storage.clone(),
    )
    .await
    .expect("could not start protocol controller");

    let protocol_command_sender = ProtocolCommandSender(protocol_command_sender);
    let (
        _network_controller,
        _protocol_command_sender,
        protocol_manager,
        _graph_event_receiver,
        _protocol_pool_event_receiver,
    ) = test(
        network_controller,
        protocol_command_sender,
        protocol_manager,
        mock_graph_receiver,
        mock_pool_receiver,
        storage,
    )
    .await;

    protocol_manager
        .stop()
        .await
        .expect("Failed to shutdown protocol.");
}

/// send a block and assert it has been propagate (or not)
pub async fn send_and_propagate_block(
    network_controller: &mut MockNetworkController,
    block: WrappedBlock,
    valid: bool,
    source_node_id: NodeId,
    protocol_graph_event_receiver: &mut GraphEventReceiver,
    protocol_command_sender: &mut ProtocolCommandSender,
    operations: Vec<WrappedOperation>,
) {
    let expected_hash = block.id;

    network_controller
        .send_header(source_node_id, block.content.header.clone())
        .await;

    protocol_command_sender
        .send_wishlist_delta(
            vec![(block.id, Some(block.content.header.clone()))]
                .into_iter()
                .collect(),
            PreHashSet::<BlockId>::default(),
        )
        .unwrap();

    // Send block info to protocol.
    let info = vec![(
        block.id,
        BlockInfoReply::Info(block.content.operations.clone()),
    )];
    network_controller
        .send_block_info(source_node_id, info)
        .await;

    // Send full ops.
    let info = vec![(block.id, BlockInfoReply::Operations(operations))];
    network_controller
        .send_block_info(source_node_id, info)
        .await;

    // Check protocol sends block to consensus.
    let hash =
        protocol_graph_event_receiver.wait_command(MassaTime::from_millis(1000), |command| {
            match command {
                MockGraphControllerMessage::RegisterBlock {
                    block_id,
                    slot: _,
                    block_storage: _,
                } => Some(block_id),
                _ => panic!("Unexpected or no protocol event."),
            }
        });
    if valid {
        assert_eq!(expected_hash, hash.unwrap());
    } else {
        assert!(hash.is_none(), "unexpected protocol event")
    }
}
