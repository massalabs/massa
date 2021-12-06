use assert_cmd::Command;
use serial_test::serial;
use std::thread;

#[tokio::test]
#[serial]
async fn test_if_exit_gracefully() {
    let mut cmd = Command::cargo_bin("massa-client").unwrap();
    cmd.arg("exit").assert().success();
}

const CONFIG_PATH: &str = "../massa-node/src/tests/config.toml";
const TIMEOUT: u64 = 5;
#[tokio::test]
#[serial]
async fn test_if_node_stop() {
    let massa_node_thread_handle = thread::spawn(|| {
        Command::cargo_bin("massa-node")
            .unwrap()
            .env("MASSA_CONFIG_PATH", CONFIG_PATH)
            .timeout(std::time::Duration::from_secs(61))
            .assert()
            .success();
    });
    let mut cmd = Command::cargo_bin("massa-client").unwrap();
    let mut success = false;
    for _ in 0..5 {
        tokio::time::sleep(std::time::Duration::from_secs(TIMEOUT)).await;
        let assert_cli = cmd.arg("node_stop").assert();
        let output = std::str::from_utf8(&assert_cli.get_output().stdout).unwrap();
        if !output.contains("tcp connect error") {
            println!("Client output: {}", output);
            success = true;
            break;
        }
    }
    massa_node_thread_handle.join().unwrap();
    assert!(success, "Error: Failed to close correctly the node 3 times");
    cmd.arg("exit").assert().success();
}
