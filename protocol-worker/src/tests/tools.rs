use crate::start_protocol_controller;
use futures::Future;
use protocol_exports::{
    tests::mock_network_controller::MockNetworkController, ProtocolCommandSender,
    ProtocolEventReceiver, ProtocolManager, ProtocolPoolEventReceiver, ProtocolSettings,
};

pub async fn protocol_test<F, V>(protocol_settings: &'static ProtocolSettings, test: F)
where
    F: FnOnce(
        MockNetworkController,
        ProtocolEventReceiver,
        ProtocolCommandSender,
        ProtocolManager,
        ProtocolPoolEventReceiver,
    ) -> V,
    V: Future<
        Output = (
            MockNetworkController,
            ProtocolEventReceiver,
            ProtocolCommandSender,
            ProtocolManager,
            ProtocolPoolEventReceiver,
        ),
    >,
{
    let (network_controller, network_command_sender, network_event_receiver) =
        MockNetworkController::new();

    // start protocol controller
    let (
        protocol_command_sender,
        protocol_event_receiver,
        protocol_pool_event_receiver,
        protocol_manager,
    ) = start_protocol_controller(
        &protocol_settings,
        5u64,
        network_command_sender,
        network_event_receiver,
    )
    .await
    .expect("could not start protocol controller");

    let (
        _network_controller,
        protocol_event_receiver,
        _protocol_command_sender,
        protocol_manager,
        protocol_pool_event_receiver,
    ) = test(
        network_controller,
        protocol_event_receiver,
        protocol_command_sender,
        protocol_manager,
        protocol_pool_event_receiver,
    )
    .await;

    protocol_manager
        .stop(protocol_event_receiver, protocol_pool_event_receiver)
        .await
        .expect("Failed to shutdown protocol.");
}
