//RUST_BACKTRACE=1 cargo test test_one_handshake -- --nocapture --test-threads=1

use super::super::{
    default_protocol_controller::DefaultProtocolController, handshake_worker::*,
    protocol_controller::ProtocolController,
};
use super::{
    mock_network_controller::{self, MockNetworkCommand, MockNetworkController},
    tools,
};
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
    let protocol_config = tools::create_protocol_config();
    let (network_controller, mut network_controler_interface) = mock_network_controller::new();

    // start protocol controller
    let protocol = DefaultProtocolController::new(protocol_config, network_controller)
        .await
        .unwrap();

    // establish a full connection with the controller
    {
        // notify the controller of a new connection
        let (mock_sock_read, mock_sock_write, conn_id) =
            network_controler_interface.new_connection().await;

        // perform handshake with controller
        let (_controller_node_id, _mock_reader, _mock_writer) =
            HandshakeWorker::<MockNetworkController>::new(
                mock_sock_read,
                mock_sock_write,
                mock_node_id,
                mock_private_key,
                Duration::from_millis(1000),
            )
            .run()
            .await
            .expect("handshake failed");

        // wait for the controller to signal that the connection is Alive
        // note: ignore peer list requests
        let notified_conn_id = timeout(Duration::from_millis(1000), async {
            loop {
                match network_controler_interface.wait_command().await {
                    Some(MockNetworkCommand::ConnectionAlive(c_id)) => return c_id,
                    Some(MockNetworkCommand::MergeAdvertisedPeerList(_)) => {}
                    Some(MockNetworkCommand::GetAdvertisablePeerList(resp_tx)) => {
                        resp_tx.send(vec![]).unwrap()
                    }
                    evt @ Some(_) => panic!("controller sent unexpected event: {:?}", evt),
                    None => panic!("controller exited unexpectedly"),
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
    tools::ignore_commands_while(protocol.stop(), &mut network_controler_interface).await;
}
