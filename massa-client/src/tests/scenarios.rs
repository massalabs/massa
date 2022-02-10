//! Tested only with feature "local"
use anyhow::{bail, Result};
use assert_cmd::Command;
use massa_models::api::NodeStatus;
use serde::de::DeserializeOwned;
use serial_test::serial;
use std::{thread::JoinHandle, time::Duration};

const TIMEOUT: u64 = 30;
const TENTATIVES: u64 = 5;

async fn send_cmd(cmd: &str) -> Result<String> {
    for _ in 0..TENTATIVES {
        let output = Command::new("cargo")
            .args(&["run", "--", cmd, "--json"])
            .assert()
            .get_output()
            .clone();
        println!("{}", std::str::from_utf8(&output.stderr).unwrap());
        let stdout = std::str::from_utf8(&output.stdout).unwrap().to_string();
        if !stdout.contains("tcp connect error") {
            return Ok(stdout);
        }
        tokio::time::sleep(std::time::Duration::from_secs(TIMEOUT)).await;
    }
    bail!("was not able to send command")
}

async fn send_cmd_without_output(cmd: &str) -> Result<()> {
    send_cmd(cmd).await?;
    Ok(())
}

async fn send_cmd_with_output<T: DeserializeOwned>(cmd: &str) -> Result<T> {
    let stdout = send_cmd(cmd).await?;
    Ok(serde_json::from_str::<T>(&stdout)?)
}

fn spawn_node(timeout: Duration) -> JoinHandle<String> {
    std::thread::spawn(move || {
        let output = Command::new("cargo")
            .args(["run", "--features", "test"])
            .current_dir("../massa-node")
            // TODO: .env("MASSA_CONFIG_PATH", "../massa-node/src/tests/config.toml");
            // use custom wallet / staking adresses / peers to not pollute user files
            .timeout(timeout)
            .assert()
            .get_output()
            .clone();
        println!("{}", std::str::from_utf8(&output.stderr).unwrap());
        std::str::from_utf8(&output.stdout).unwrap().to_string()
    })
}

#[tokio::test]
#[serial]
async fn test_run_node() {
    let handle = spawn_node(Duration::from_secs(60 * 5));

    let output = send_cmd_with_output::<NodeStatus>("get_status")
        .await
        .unwrap();
    // TODO: assert_eq! or try a REGEX against the receved output to validate it
    println!("{}", output);

    ////////////////////////////////////////////////////
    // TODO: test other API endpoints/client commands //
    ////////////////////////////////////////////////////

    send_cmd_without_output("node_stop").await.unwrap();

    // Check that `massa-node` did stop
    let output = handle.join().expect("did not succeed to close the node");
    // TODO: assert_eq! or try a REGEX against the node log log to check that we stop gracefully
    println!("{}", output);

    send_cmd_without_output("exit").await.unwrap();
}
