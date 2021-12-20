use std::{
    thread::{sleep, JoinHandle},
    time::Duration,
};

use anyhow::{bail, Result};
use assert_cmd::Command;
use serial_test::serial;

const TIMEOUT: u64 = 10;
const TENTATIVES: u64 = 5;

async fn run_client_cmd(cmd: &str) -> Result<String> {
    for _ in 0..TENTATIVES {
        let output = std::str::from_utf8(
            &Command::new("cargo")
                .args(&["run", "--", cmd, "--json"])
                .assert()
                .get_output()
                .stdout,
        )?
        .to_string();
        println!("{}", output);
        // TODO: compare with tcp connect error as a JSON object?
        if !output.contains("tcp connect error") {
            return Ok(output);
        }
        tokio::time::sleep(std::time::Duration::from_secs(TIMEOUT)).await;
    }
    bail!("was not able to send command")
}

fn run_node(duration: Duration) -> JoinHandle<String> {
    std::thread::spawn(move || {
        let out = Command::new("cargo")
            .args(["run", "--features", "test"])
            .current_dir("../massa-node")
            .timeout(duration)
            .assert()
            .get_output()
            .clone();
        println!("{}", std::str::from_utf8(&out.stderr).unwrap());
        std::str::from_utf8(&out.stdout).unwrap().to_string()
    })
}

#[tokio::test]
#[serial]
async fn test_run_node() {
    let handle = run_node(Duration::from_secs(60 * 3));
    sleep(Duration::from_secs(30)); // let it compile and start
    let a = handle.join().unwrap();
    println!("{}", a);
}

#[tokio::test]
#[serial]
async fn client_exit_gracefully() {
    run_client_cmd("exit").await.unwrap();
}

#[tokio::test]
#[serial]
async fn node_stop_gracefully() {
    let handle = run_node(Duration::from_secs(60 * 3));
    sleep(Duration::from_secs(180)); // let it compile and start
    run_client_cmd("node_stop").await.unwrap();

    // check that `massa-node` did stop
    let a = handle.join().expect("did not succeed to close the node");
}
