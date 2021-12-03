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

const CONFIG_PATH: &str = "../massa-node/src/tests/config_test.toml";

#[tokio::test]
#[serial]
async fn test_if_node_stop() {
    tools::update_genesis_timestamp(CONFIG_PATH);
    let massa_node_thread_handle = thread::spawn(|| {
        Command::cargo_bin("massa-node")
            .unwrap()
            .env("MASSA_CONFIG_PATH", CONFIG_PATH)
            .timeout(std::time::Duration::from_secs(61))
            .assert()
            .success();
    });
    tokio::time::sleep(std::time::Duration::from_secs(5)).await;
    let mut cmd = Command::cargo_bin("massa-client").unwrap();
    let mut success = false;
    for _ in 0..3 {
        tokio::time::sleep(std::time::Duration::from_secs(10)).await;
        let assert_cli = cmd.arg("node_stop").assert();
        let out = std::str::from_utf8(&assert_cli.get_output().stdout).unwrap();
        if out.contains("RpcError") {
            println!("RpcError, retry to send message");
        } else {
            println!("Message request sent: {}", out);
            success = true;
            break;
        }
    }
    massa_node_thread_handle.join().unwrap();
    assert!(success, "Error: Failed to close correctly the node 3 times");
    cmd.arg("exit").assert().success();
}
