use consensus::{ConsensusCommand, ConsensusCommandSender};
use models::SerializeCompact;
use std::str::FromStr;
use tokio::sync::mpsc;

use crate::{get_state, start_bootstrap_server};

use super::{
    mock_establisher,
    tools::{
        bridge_mock_streams, get_boot_graph, get_bootstrap_config, get_keys,
        get_serialization_context, wait_consensus_command,
    },
};

#[tokio::test]
async fn test_bootstrap_server() {
    let (command_tx, mut command_rx) = mpsc::channel::<ConsensusCommand>(5);
    let (private_key, public_key) = get_keys();

    let cfg = get_bootstrap_config(public_key);
    let (bootstrap_establisher, bootstrap_interface) = mock_establisher::new();
    let bootstrap_manager = start_bootstrap_server(
        ConsensusCommandSender(command_tx),
        cfg.clone(),
        get_serialization_context(),
        bootstrap_establisher,
        private_key,
    )
    .await
    .unwrap()
    .unwrap();

    // launch the get_state process
    let (remote_establisher, mut remote_interface) = mock_establisher::new();
    let cfg_copy = cfg.clone();
    let get_state_h = tokio::spawn(async move {
        get_state(
            cfg_copy,
            get_serialization_context(),
            remote_establisher,
            0.into(),
        )
        .await
        .unwrap()
        .unwrap()
    });

    // accept connection attempt from remote
    let (remote_r, remote_w, conn_addr, resp) = tokio::time::timeout(
        std::time::Duration::from_millis(1000),
        remote_interface.wait_connection_attempt_from_controller(),
    )
    .await
    .expect("timeout waiting for connection attempt from remote")
    .expect("error receiving connection attempt from remote");
    let expect_conn_addr = std::net::SocketAddr::new(cfg.bootstrap_ip, cfg.bootstrap_port);
    assert_eq!(
        conn_addr, expect_conn_addr,
        "client connected to wrong bootstrap ip"
    );
    resp.send(true)
        .expect("could not send connection accept to remote");

    // connect to bootstrap
    let remote_addr = std::net::SocketAddr::from_str("82.245.72.98:10000").unwrap(); // not checked
    let (bootstrap_r, bootstrap_w) = tokio::time::timeout(
        std::time::Duration::from_millis(1000),
        bootstrap_interface.connect_to_controller(&remote_addr),
    )
    .await
    .expect("timeout while connecting to bootstrap")
    .expect("could not connect to bootstrap");

    // launch bridges
    let bridge_h1 = tokio::spawn(async move {
        bridge_mock_streams(remote_r, bootstrap_w).await;
    });
    let bridge_h2 = tokio::spawn(async move {
        bridge_mock_streams(bootstrap_r, remote_w).await;
    });

    // wait for bootstrap to ask consensus for bootstrap graph, send it
    let response = match wait_consensus_command(&mut command_rx, 1000.into(), |cmd| match cmd {
        ConsensusCommand::GetBootGraph(resp) => Some(resp),
        _ => None,
    })
    .await
    {
        Some(resp) => resp,
        None => panic!("timeout waiting for get boot graph consensus command"),
    };
    let sent_graph = get_boot_graph();
    response.send(sent_graph.clone()).unwrap();

    // wait for get_state
    let recv_graph = get_state_h
        .await
        .expect("error while waiting for get_state to finish");

    // wait for bridges
    bridge_h1.await.expect("bridge 1 join failed");
    bridge_h2.await.expect("bridge 2 join failed");

    // check states
    assert_eq!(
        sent_graph
            .to_bytes_compact(&get_serialization_context())
            .unwrap(),
        recv_graph
            .to_bytes_compact(&get_serialization_context())
            .unwrap(),
        "mismatch between sent and received graphs"
    );

    // stop bootstrap server
    bootstrap_manager
        .stop()
        .await
        .expect("could not stop bootstrap server");
}
