//RUST_BACKTRACE=1 cargo test test_one_handshake -- --nocapture --test-threads=1

use super::{mock_network_controller::MockNetworkController, tools};
use crate::{
    network::NetworkCommand,
    protocol::{handshake_worker::*, start_protocol_controller},
};
use time::UTime;
use tokio::time::{timeout, Duration};

// notify controller of a new connection,
//   perform handshake
//   and wait for controller to send the ConnectionAlive command
#[tokio::test]
async fn test_new_connection() {
    // setup logging
    /*
    stderrlog::new()
        .verbosity(4)
        .timestamp(stderrlog::Timestamp::Millisecond)
        .init()
        .unwrap();
    */

    // test configuration
    let (mock_private_key, mock_node_id) = tools::generate_node_keys();
    let (protocol_config, serialization_context) = tools::create_protocol_config();
    let (mut network_controller, network_command_sender, network_event_receiver) =
        MockNetworkController::new();

    // start protocol controller
    let (_protocol_command_sender, protocol_event_receiver, protocol_manager) =
        start_protocol_controller(
            protocol_config.clone(),
            serialization_context.clone(),
            network_command_sender.clone(),
            network_event_receiver,
        )
        .await
        .expect("could not start protocol controller");

    // establish a full connection with the controller
    {
        // notify the controller of a new connection
        let (mock_sock_read, mock_sock_write, conn_id) = network_controller.new_connection().await;

        // perform handshake with controller
        let (_controller_node_id, _mock_reader, _mock_writer) = HandshakeWorker::new(
            serialization_context,
            mock_sock_read,
            mock_sock_write,
            mock_node_id,
            mock_private_key,
            UTime::from(1000),
        )
        .run()
        .await
        .expect("handshake failed");

        // wait for the controller to signal that the connection is Alive
        // note: ignore peer list requests
        let notified_conn_id = timeout(Duration::from_millis(1000), async {
            loop {
                match network_controller.wait_command().await {
                    Some(NetworkCommand::ConnectionAlive(c_id)) => return c_id,
                    Some(NetworkCommand::MergeAdvertisedPeerList(_)) => {}
                    Some(NetworkCommand::GetAdvertisablePeerList(resp_tx)) => {
                        resp_tx.send(vec![]).unwrap()
                    }
                    evt @ Some(_) => panic!("controller sent unexpected event: {:?}", evt),
                    None => panic!("controller exited unexpectedly"), // in a loop
                }
            }
        })
        .await
        .expect("timeout while waiting for controller to call ConnectionAlive");
        assert_eq!(
            conn_id, notified_conn_id,
            "mismatch between the notified connection ID and the expected one"
        );
    }

    // stop controller while ignoring all commands
    let stop_fut = protocol_manager.stop(protocol_event_receiver);
    tokio::pin!(stop_fut);
    network_controller
        .ignore_commands_while(stop_fut)
        .await
        .unwrap();
}
