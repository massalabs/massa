use crate::start_protocol_controller;
use futures::Future;
use massa_pool_exports::test_exports::{MockPoolController, PoolEventReceiver};
use massa_protocol_exports::{
    tests::mock_network_controller::MockNetworkController, ProtocolCommandSender, ProtocolConfig,
    ProtocolEventReceiver, ProtocolManager,
};
use massa_storage::Storage;

pub async fn protocol_test<F, V>(protocol_config: &ProtocolConfig, test: F)
where
    F: FnOnce(
        MockNetworkController,
        ProtocolEventReceiver,
        ProtocolCommandSender,
        ProtocolManager,
        PoolEventReceiver,
    ) -> V,
    V: Future<
        Output = (
            MockNetworkController,
            ProtocolEventReceiver,
            ProtocolCommandSender,
            ProtocolManager,
            PoolEventReceiver,
        ),
    >,
{
    let (network_controller, network_command_sender, network_event_receiver) =
        MockNetworkController::new();

    let (pool_controller, pool_event_receiver) = MockPoolController::new_with_receiver();

    // start protocol controller
    let (protocol_command_sender, protocol_event_receiver, protocol_manager): (
        ProtocolCommandSender,
        ProtocolEventReceiver,
        ProtocolManager,
    ) = start_protocol_controller(
        *protocol_config,
        network_command_sender,
        network_event_receiver,
        pool_controller,
        Storage::create_root(),
    )
    .await
    .expect("could not start protocol controller");

    let (
        _network_controller,
        protocol_event_receiver,
        _protocol_command_sender,
        protocol_manager,
        _pool_event_receiver,
    ) = test(
        network_controller,
        protocol_event_receiver,
        protocol_command_sender,
        protocol_manager,
        pool_event_receiver,
    )
    .await;

    protocol_manager
        .stop(protocol_event_receiver)
        .await
        .expect("Failed to shutdown protocol.");
}

pub async fn protocol_test_with_storage<F, V>(protocol_config: &ProtocolConfig, test: F)
where
    F: FnOnce(
        MockNetworkController,
        ProtocolEventReceiver,
        ProtocolCommandSender,
        ProtocolManager,
        PoolEventReceiver,
        Storage,
    ) -> V,
    V: Future<
        Output = (
            MockNetworkController,
            ProtocolEventReceiver,
            ProtocolCommandSender,
            ProtocolManager,
            PoolEventReceiver,
        ),
    >,
{
    let (network_controller, network_command_sender, network_event_receiver) =
        MockNetworkController::new();
    let (pool_controller, mock_pool_receiver) = MockPoolController::new_with_receiver();
    let storage = Storage::create_root();
    // start protocol controller
    let (protocol_command_sender, protocol_event_receiver, protocol_manager) =
        start_protocol_controller(
            *protocol_config,
            network_command_sender,
            network_event_receiver,
            pool_controller,
            storage.clone(),
        )
        .await
        .expect("could not start protocol controller");

    let (
        _network_controller,
        protocol_event_receiver,
        _protocol_command_sender,
        protocol_manager,
        _protocol_pool_event_receiver,
    ) = test(
        network_controller,
        protocol_event_receiver,
        protocol_command_sender,
        protocol_manager,
        mock_pool_receiver,
        storage,
    )
    .await;

    protocol_manager
        .stop(protocol_event_receiver)
        .await
        .expect("Failed to shutdown protocol.");
}
