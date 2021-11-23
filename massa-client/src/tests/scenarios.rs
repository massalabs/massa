use super::tools;
use assert_cmd::Command;
use serial_test::serial;
use std::thread;

#[tokio::test]
#[serial]
async fn test_if_exit_gracefully() {
    let mut cmd = Command::cargo_bin("massa-client").unwrap();
    cmd.arg("exit").assert().success();
}

const CONFIG_PATH: &str = "../massa-node/base_config/testnet.toml";

#[tokio::test]
#[serial]
async fn test_if_wallet_info() {
    tools::update_genesis_timestamp(CONFIG_PATH);
    let massa_node_thread_handle = thread::spawn(|| {
        Command::cargo_bin("massa-node")
            .unwrap()
            .env("MASSA_CONFIG_PATH", CONFIG_PATH)
            .unwrap();
    });
    let mut cmd = Command::cargo_bin("massa-client").unwrap();
    thread::sleep(std::time::Duration::from_secs(5));
    cmd.arg("node_stop").assert().success();
    massa_node_thread_handle.join().unwrap();
    cmd.arg("exit").assert().success();
}
